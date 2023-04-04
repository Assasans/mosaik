pub mod track;

use std::sync::Arc;
use anyhow::{Result, Context};
use songbird::{Call, tracks::TrackHandle};
use tokio::sync::Mutex;
use twilight_model::id::{Id, marker::{GuildMarker, ChannelMarker}};

use crate::State;

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

#[derive(Debug)]
pub struct Player {
  pub state: State,
  pub call: Option<Arc<Mutex<Call>>>,
  pub handle: Option<TrackHandle>,

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
      call: None,
      handle: None,

      guild_id,
      channel_id: None,

      player_state: PlayerState::Stop,
      repeat_type: RepeatType::None,

      tracks: Vec::new(),
      current: 0
    }
  }

  pub async fn play(&mut self, index: usize) -> Result<&Track> {
    let mut call = self.call.as_ref().context("no call")?.lock().await;
    let track = self.tracks.get(index).context("no track")?;
    self.current = index;
    self.handle = Some(call.play_only_input(track.provider.to_input().await?));

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
