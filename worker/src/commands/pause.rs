use anyhow::Result;

use crate::{AnyError, PoiseContext};

#[poise::command(prefix_command, track_edits, slash_command)]
pub async fn pause(ctx: PoiseContext<'_>) -> Result<(), AnyError> {
  ctx.reply("Processing...").await?;

  let guild_id = ctx.guild_id().unwrap();

  let state = ctx.data();
  let players = state.players.read().await;
  let player = if let Some(player) = players.get(&guild_id) {
    player
  } else {
    ctx.reply("No player").await?;
    return Ok(());
  };

  player.connection.set_paused(!player.connection.is_paused());
  ctx.reply("Ok").await?;

  Ok(())
}
