pub mod track;

use std::sync::Arc;

use anyhow::{Result, Context};
use futures_util::StreamExt;
use symphonia::core::probe::Hint;
use tracing::debug;
use twilight_gateway::{Event, EventType};
use twilight_model::{id::{Id, marker::{GuildMarker, ChannelMarker}}, gateway::payload::outgoing::UpdateVoiceState};

use voice::{VoiceConnectionOptions, VoiceConnection};
use crate::{State, voice::SymphoniaSampleProvider};
use self::track::Track;

#[derive(Debug)]
pub enum RepeatType {
  None,
  Player,
  Track
}

#[derive(Debug)]
pub enum PlayerState {
  Stop,
  Pause,
  Play
}

pub struct Player {
  pub state: State,
  pub connection: Option<Arc<VoiceConnection>>,

  pub guild_id: Id<GuildMarker>,
  pub channel_id: Option<Id<ChannelMarker>>,

  pub player_state: PlayerState,
  pub repeat_type: RepeatType,

  pub tracks: Vec<Track>,
  pub current: usize
}

impl Player {
  pub fn new(state: State, guild_id: Id<GuildMarker>) -> Self {
    Self {
      state,
      connection: None,

      guild_id,
      channel_id: None,

      player_state: PlayerState::Stop,
      repeat_type: RepeatType::None,

      tracks: Vec::new(),
      current: 0
    }
  }

  pub fn set_channel(&mut self, channel_id: Id<ChannelMarker>) {
    self.channel_id = Some(channel_id);
  }

  pub async fn connect(&mut self) -> Result<()> {
    let channel_id = self.channel_id.context("no voice channel")?;

    self.state.sender.command(&UpdateVoiceState::new(self.guild_id, channel_id, true, false))?;

    let mut voice_state = None;
    let mut voice_server = None;

    let mut stream = self.state.standby.wait_for_stream(self.guild_id, |event: &Event| match event.kind() {
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
      guild_id: self.guild_id.get(),
      bitrate: channel.bitrate,
      endpoint: voice_server.endpoint.context("no voice endpoint")?.to_owned(),
      token: voice_server.token.to_owned(),
      session_id: voice_state.session_id.to_owned()
    };
    let connection = Arc::new(VoiceConnection::new()?);
    connection.connect(options).await?;

    let connection_weak = Arc::downgrade(&connection);
    tokio::spawn(async move {
      VoiceConnection::run_ws_loop(connection_weak).await.unwrap()
    });

    // TODO(Assasans): Internal code
    {
      let mut ws_lock = connection.ws.lock().await;
      ws_lock.as_mut().unwrap().send_speaking(true).await?;
    }

    let file = std::fs::File::open("/home/assasans/Downloads/[Hi-Res] Chiisana Boukensha by Aqua, Megumin and Darkness/01 ちいさな冒険者 [ORT].flac")?;
    let mut hint = Hint::new();
    hint.with_extension("flac");

    *connection.sample_provider.lock().await = Some(Box::new(SymphoniaSampleProvider::new_from_source(Box::new(file), hint)?));

    let clone = connection.clone();
    tokio::spawn(async move {
      VoiceConnection::run_udp_loop(clone).await.unwrap();
    });

    self.connection = Some(connection);

    Ok(())
  }

  pub async fn play(&mut self, index: usize) -> Result<&Track> {
    todo!();
    let track = self.tracks.get(index).context("no track")?;
    self.current = index;

    Ok(track)
  }

  pub fn get_current_track(&self) -> Option<&Track> {
    self.tracks.get(self.current)
  }

  pub fn get_next_track(&self) -> Option<&Track> {
    match self.repeat_type {
      RepeatType::None => self.tracks.get(self.current + 1),
      RepeatType::Track => self.get_current_track(),
      RepeatType::Player => {
        if !self.tracks.is_empty() {
          let index = if self.current == self.tracks.len() - 1 {
            0
          } else {
            self.current
          };

          self.tracks.get(index + 1)
        } else {
          None
        }
      }
    }
  }

  pub fn get_previous_track(&self) -> Option<&Track> {
    match self.repeat_type {
      RepeatType::Player => {
        if !self.tracks.is_empty() {
          let index = if self.current == 0 {
            self.tracks.len()
          } else {
            self.current
          };

          self.tracks.get(index - 1)
        } else {
          None
        }
      },
      _ => self.tracks.get(self.current - 1),
    }
  }
}
