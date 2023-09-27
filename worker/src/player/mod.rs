pub mod track;
pub mod queue;

use std::sync::{Arc, RwLock};
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::{Result, Context, anyhow};
use futures_util::StreamExt;
use tokio::time;
use tracing::{debug, warn};
use twilight_gateway::{Event, EventType};
use twilight_model::{id::{Id, marker::{GuildMarker, ChannelMarker}}, gateway::payload::outgoing::UpdateVoiceState};

use voice::{VoiceConnectionOptions, VoiceConnection, VoiceConnectionState};
use crate::player::queue::Queue;
use crate::State;

pub enum PlayerEvent {
  TrackFinished(usize)
}

pub struct Player {
  pub state: State,
  pub connection: Arc<VoiceConnection>,

  pub guild_id: RwLock<Id<GuildMarker>>,
  pub channel_id: RwLock<Option<Id<ChannelMarker>>>,

  pub queue: Arc<Queue>,

  pub tx: flume::Sender<PlayerEvent>,
  pub rx: flume::Receiver<PlayerEvent>
}

impl Player {
  pub fn new(state: State, guild_id: Id<GuildMarker>) -> Self {
    let (tx, rx) = flume::bounded(16);

    Self {
      state,
      connection: Arc::new(VoiceConnection::new().unwrap()),

      guild_id: RwLock::new(guild_id),
      channel_id: RwLock::new(None),

      queue: Queue::new(),

      tx,
      rx
    }
  }

  pub fn set_channel(&self, channel_id: Id<ChannelMarker>) {
    *self.channel_id.write().unwrap() = Some(channel_id);
  }

  pub fn get_channel(&self) -> Option<Id<ChannelMarker>> {
    *self.channel_id.read().unwrap()
  }

  pub fn get_guild(&self) -> Id<GuildMarker> {
    *self.guild_id.read().unwrap()
  }

  pub async fn connect(self: &Arc<Self>) -> Result<()> {
    let channel_id = self.get_channel().context("no voice channel")?;
    self.state.sender.command(&UpdateVoiceState::new(self.get_guild(), channel_id, true, false))?;

    let mut voice_state = None;
    let mut voice_server = None;

    let mut stream = self.state.standby.wait_for_stream(self.get_guild(), |event: &Event| match event.kind() {
      EventType::VoiceStateUpdate => true,
      EventType::VoiceServerUpdate => true,
      _ => false
    });

    while let Some(event) = stream.next().await {
      match event {
        Event::VoiceStateUpdate(vs) => voice_state = Some(vs),
        Event::VoiceServerUpdate(vs) => voice_server = Some(vs),
        _ => {}
      }

      if voice_state.is_some() && voice_server.is_some() {
        break;
      }
    };
    let voice_state = voice_state.unwrap();
    let voice_server = voice_server.unwrap();

    debug!(?voice_state, ?voice_server, "got connection info");

    let user = self.state.cache.current_user().context("no current user")?;
    let channel = self.state.cache.channel(channel_id).context("no channel cached")?;

    let options = VoiceConnectionOptions {
      user_id: user.id.get(),
      guild_id: self.get_guild().get(),
      bitrate: channel.bitrate,
      endpoint: voice_server.endpoint.context("no voice endpoint")?.to_owned(),
      token: voice_server.token.to_owned(),
      session_id: voice_state.session_id.to_owned()
    };
    self.connection.connect(options).await?;

    let connection_weak = Arc::downgrade(&self.connection);
    tokio::spawn(async move {
      loop {
        match VoiceConnection::run_ws_loop(connection_weak.clone()).await {
          Ok(()) => {
            debug!("VoiceConnection::run_ws_loop clean exit");
            break;
          }
          Err(error) => {
            warn!("VoiceConnection::run_ws_loop error: {:?}", error);
            time::sleep(Duration::from_millis(3000)).await;

            loop {
              match connection_weak.upgrade().unwrap().reconnect_ws().await {
                Ok(()) => break,
                Err(error) => {
                  warn!("VoiceConnection::reconnect_ws error: {:?}", error);
                  time::sleep(Duration::from_millis(3000)).await;
                }
              }
            }
          }
        }
      }

      if let Some(connection) = connection_weak.upgrade() {
        connection.stop_udp_loop.store(true, Ordering::Relaxed);
      }
    });

    let cloned = self.clone();
    let rx = self.rx.clone();
    tokio::spawn(async move {
      loop {
        match rx.recv_async().await.unwrap() {
          PlayerEvent::TrackFinished(position) => {
            let next = {
              let mode = cloned.queue.mode.read().unwrap();
              mode.seek(1, false)
            };
            debug!("track {} finished, next {:?}", position, next);

            if let Some(next) = next {
              cloned.queue.set_position(next);
              cloned.play().await.unwrap();
            }
          }
        }
      }
    });

    Ok(())
  }

  pub async fn stop(self: &Arc<Self>) -> Result<()> {
    if self.connection.state.get() != VoiceConnectionState::Playing {
      return Err(anyhow!("invalid player state (expected playing)"));
    }
    self.connection.stop_udp_loop.store(true, Ordering::Relaxed);

    debug!("waiting for udp loop to exit...");
    self.connection.state.wait_for(|state| *state != VoiceConnectionState::Playing).await;

    Ok(())
  }

  pub async fn play(self: &Arc<Self>) -> Result<()> {
    if self.connection.state.get() == VoiceConnectionState::Playing {
      return Err(anyhow!("invalid player state (playing)"));
    }

    debug!("playing track {} / {}", self.queue.position(), self.queue.len());
    let track = self.queue.get_current().upgrade().unwrap();

    let sample_provider = track.provider.get_sample_provider().await?;
    *self.connection.sample_provider_handle.lock().await = Some(sample_provider.get_handle());
    *self.connection.sample_provider.lock().await = Some(sample_provider);

    let x = self.clone();
    let clone = self.connection.clone();
    tokio::spawn(async move {
      VoiceConnection::run_udp_loop(clone).await.unwrap();

      // If stop_udp_loop is not set - send PlayerEvent::TrackFinished
      if let Err(_) = x.connection.stop_udp_loop.compare_exchange(true, false, Ordering::Acquire, Ordering::Relaxed) {
        x.tx.send_async(PlayerEvent::TrackFinished(x.queue.position())).await.unwrap();
      }
    });

    Ok(())
  }
}
