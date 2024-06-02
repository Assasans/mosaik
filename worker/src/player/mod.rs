pub mod queue;
pub mod track;

use std::sync::atomic::Ordering;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde_json::json;
use serenity::all::{Cache, ChannelId, CreateMessage, GuildId, MessageBuilder};
use serenity::constants::Opcode;
use serenity::gateway::{ShardMessenger, ShardRunnerMessage};
use tokio::sync::oneshot;
use tokio::time;
use tracing::{debug, info, warn};
use voice::{VoiceConnection, VoiceConnectionEvent, VoiceConnectionOptions, VoiceConnectionState};

use crate::player::queue::Queue;
use crate::voice::MosaikVoiceManager;
use crate::{PoiseContext, State};

pub enum PlayerEvent {
  TrackFinished(usize)
}

pub struct Player {
  pub state: State,
  pub connection: Arc<VoiceConnection>,

  pub guild_id: RwLock<GuildId>,
  pub context: tokio::sync::RwLock<Option<serenity::client::Context>>,
  pub text_channel_id: RwLock<Option<ChannelId>>,
  pub channel_id: RwLock<Option<ChannelId>>,

  pub queue: Arc<Queue>,

  pub tx: flume::Sender<PlayerEvent>,
  pub rx: flume::Receiver<PlayerEvent>
}

impl Player {
  pub fn new(state: State, guild_id: GuildId) -> Self {
    let (tx, rx) = flume::bounded(16);

    Self {
      state,
      connection: Arc::new(VoiceConnection::new().unwrap()),

      guild_id: RwLock::new(guild_id),
      context: tokio::sync::RwLock::new(None),
      text_channel_id: RwLock::new(None),
      channel_id: RwLock::new(None),

      queue: Queue::new(),

      tx,
      rx
    }
  }

  pub fn set_channel(&self, channel_id: ChannelId) {
    *self.channel_id.write().unwrap() = Some(channel_id);
  }

  pub async fn set_context(&self, context: serenity::client::Context) {
    *self.context.write().await = Some(context);
  }

  pub fn set_text_channel_id(&self, channel_id: ChannelId) {
    *self.text_channel_id.write().unwrap() = Some(channel_id);
  }

  pub fn get_channel(&self) -> Option<ChannelId> {
    *self.channel_id.read().unwrap()
  }

  pub fn get_guild(&self) -> GuildId {
    *self.guild_id.read().unwrap()
  }

  pub async fn connect(
    self: &Arc<Self>,
    voice_manager: &MosaikVoiceManager,
    cache: &Cache,
    shard: &ShardMessenger
  ) -> Result<()> {
    let guild_id = self.get_guild();
    let channel_id = self.get_channel().context("no voice channel")?;

    let (tx, rx) = oneshot::channel();
    voice_manager.invalidate_state(&guild_id).await; // TODO: Invalidate as soon as disconnected
    voice_manager.callbacks.write().await.insert(guild_id, tx);

    // Serenity...
    shard.send_to_shard(ShardRunnerMessage::Message(
      serde_json::to_string(&json!({
        "op": Opcode::VoiceStateUpdate,
        "d": {
          "guild_id": guild_id,
          "channel_id": channel_id,
          "self_mute": false,
          "self_deaf": true
        }
      }))?
      .into()
    ));

    let state = rx.await.unwrap();
    debug!(?state, "got connection info");

    let options = VoiceConnectionOptions {
      user_id: cache.current_user().id.get(),
      guild_id: self.get_guild().get(),
      bitrate: cache.channel(channel_id).context("no channel cached")?.bitrate,
      endpoint: state.endpoint.context("no voice endpoint")?,
      token: state.token.unwrap(),
      session_id: state.session_id.unwrap()
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
    self
      .connection
      .state
      .wait_for(|state| *state != VoiceConnectionState::Playing)
      .await;

    Ok(())
  }

  pub async fn play(self: &Arc<Self>) -> Result<()> {
    if self.connection.state.get() == VoiceConnectionState::Playing {
      return Err(anyhow!("invalid player state (playing)"));
    }

    debug!("playing track {} / {}", self.queue.position(), self.queue.len());
    let track = self.queue.get_current().upgrade().unwrap();

    let sample_provider = track.provider.get_sample_provider().await?;
    debug!("initializing sample provider (deadlock test)");
    *self.connection.sample_provider_handle.lock().await = Some(sample_provider.get_handle());
    *self.connection.sample_provider.lock().unwrap() = Some(sample_provider);
    debug!("sample provider initialized (deadlock test)");

    let x = self.clone();
    let clone = self.connection.clone();
    tokio::spawn(async move {
      VoiceConnection::run_udp_loop(clone).await.unwrap();

      // If stop_udp_loop is not set - send PlayerEvent::TrackFinished
      if let Err(_) = x
        .connection
        .stop_udp_loop
        .compare_exchange(true, false, Ordering::Acquire, Ordering::Relaxed)
      {
        x.tx
          .send_async(PlayerEvent::TrackFinished(x.queue.position()))
          .await
          .unwrap();
      }
    });

    let clone = self.clone();
    tokio::spawn(async move {
      while let Ok(event) = clone.connection.events.recv_async().await {
        info!("voice event: {:?}", event);
        match event {
          VoiceConnectionEvent::RmsPeak(rms) => {
            info!("rms peak: {}", rms);
            if let Some(context) = &*clone.context.read().await {
              let channel_id = clone.text_channel_id.read().unwrap().unwrap();
              channel_id.send_message(context, CreateMessage::new().content(format!("RMS peaked at `{}`, playback was paused.", rms))).await.unwrap();
            }
          }
        }
      }
    });

    Ok(())
  }
}
