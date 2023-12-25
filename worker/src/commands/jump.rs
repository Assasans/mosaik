use anyhow::{Context, Result};
use tracing::debug;
use voice::VoiceConnectionState;

use crate::state::get_player_or_fail;
use crate::{AnyError, PoiseContext};

#[poise::command(prefix_command, track_edits, slash_command)]
pub async fn jump(
  ctx: PoiseContext<'_>,
  #[description = "Specific command to show help about"]
  #[autocomplete = "poise::builtins::autocomplete_command"]
  position: String
) -> Result<(), AnyError> {
  ctx.reply("Processing...").await?;

  let player = get_player_or_fail!(ctx);

  debug!("jump: {}", position);
  let current_position = player.queue.position();

  let position = match position.chars().nth(0).context("no first position character")? {
    '+' => current_position + position[1..].parse::<usize>()?,
    '-' => current_position.saturating_sub(position[1..].parse::<usize>()?),
    _ => position.parse::<usize>()?
  };

  if player.connection.state.get() == VoiceConnectionState::Playing {
    player.stop().await?;
  }
  player.queue.set_position(position);
  player.play().await?;

  ctx
    .reply(format!("Jumped to track {:?} (was: {:?})", position, current_position))
    .await?;

  Ok(())
}
