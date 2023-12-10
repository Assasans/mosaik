use std::collections::HashMap;
use std::sync::Arc;

use serenity::all::GuildId;
use tokio::sync::RwLock;

use crate::player::Player;

pub type State = Arc<StateRef>;

pub struct StateRef {
  pub players: RwLock<HashMap<GuildId, Arc<Player>>>
}

macro_rules! get_player_or_fail {
  ($ctx:expr) => {{
    use ::anyhow::Context;

    let guild_id = $ctx.guild_id().context("no guild_id")?;
    let state = $ctx.data();
    let players = state.players.read().await;
    if let Some(player) = players.get(&guild_id) {
      player.clone()
    } else {
      $ctx.reply("No player").await?;
      return Ok(());
    }
  }};
}

pub(crate) use get_player_or_fail;
