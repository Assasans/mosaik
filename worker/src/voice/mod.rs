use std::collections::HashMap;

use async_trait::async_trait;
use futures_channel::mpsc::UnboundedSender;
use serenity::all::{ChannelId, GuildId, ShardRunnerMessage, UserId, VoiceGatewayManager, VoiceState};
use tokio::sync::oneshot::Sender;
use tokio::sync::RwLock;
use tracing::{debug, info};

pub mod ffmpeg;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MosaikVoiceState {
  pub guild_id: GuildId,
  pub channel_id: Option<ChannelId>,
  pub session_id: Option<String>,
  pub endpoint: Option<String>,
  pub token: Option<String>
}

impl MosaikVoiceState {
  pub fn new(guild_id: GuildId) -> Self {
    Self {
      guild_id,
      channel_id: None,
      session_id: None,
      endpoint: None,
      token: None
    }
  }
}

#[derive(Debug)]
pub struct MosaikVoiceManager {
  pub states: RwLock<HashMap<GuildId, MosaikVoiceState>>,
  pub callbacks: RwLock<HashMap<GuildId, Sender<MosaikVoiceState>>>
}

impl MosaikVoiceManager {
  pub fn new() -> Self {
    Self {
      states: Default::default(),
      callbacks: Default::default()
    }
  }

  async fn run_callback_if_needed(&self, state: &MosaikVoiceState) {
    if state.session_id.is_some() && state.endpoint.is_some() && state.token.is_some() {
      let mut callbacks = self.callbacks.write().await;
      if let Some(callback) = callbacks.remove(&state.guild_id) {
        callback.send(state.to_owned()).unwrap();
        debug!("run callback for {:?}", state);
      }
    }
  }

  pub async fn invalidate_state(&self, guild_id: &GuildId) -> Option<MosaikVoiceState> {
    let mut states = self.states.write().await;
    states.remove(guild_id)
  }
}

#[async_trait]
impl VoiceGatewayManager for MosaikVoiceManager {
  async fn initialise(&self, shard_count: u32, user_id: UserId) {
    info!(?user_id, ?shard_count, "voice manager initialized");
  }

  async fn register_shard(&self, shard_id: u32, _sender: UnboundedSender<ShardRunnerMessage>) {
    info!(?shard_id, "register shard");
  }

  async fn deregister_shard(&self, shard_id: u32) {
    info!(?shard_id, "deregister shard");
  }

  async fn server_update(&self, guild_id: GuildId, endpoint: &Option<String>, token: &str) {
    let mut states = self.states.write().await;
    let state = states
      .entry(guild_id)
      .or_insert_with(|| MosaikVoiceState::new(guild_id));
    state.endpoint = endpoint.clone();
    state.token = Some(token.to_owned());
    debug!("voice server update: {:?}", state);
    self.run_callback_if_needed(&state).await;
  }

  async fn state_update(&self, guild_id: GuildId, voice_state: &VoiceState) {
    let mut states = self.states.write().await;
    let state = states
      .entry(guild_id)
      .or_insert_with(|| MosaikVoiceState::new(guild_id));
    state.channel_id = voice_state.channel_id;
    state.session_id = Some(voice_state.session_id.clone());
    debug!("voice state update: {:?}", state);
    self.run_callback_if_needed(&state).await;
  }
}
