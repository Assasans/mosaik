use anyhow::Result;

use crate::state::get_player_or_fail;
use crate::{AnyError, PoiseContext};

#[poise::command(prefix_command, track_edits, slash_command)]
pub async fn pause(ctx: PoiseContext<'_>) -> Result<(), AnyError> {
  ctx.reply("Processing...").await?;

  let player = get_player_or_fail!(ctx);

  player.connection.set_paused(!player.connection.is_paused());
  ctx.reply("Ok").await?;

  Ok(())
}
