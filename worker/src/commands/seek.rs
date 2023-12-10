
use std::time::Duration;
use anyhow::{Context, Result};

use tracing::debug;

use crate::{PoiseContext, AnyError};
use crate::state::get_player_or_fail;
use crate::voice::ffmpeg::FFmpegSampleProviderHandle;

#[poise::command(prefix_command, track_edits, slash_command)]
pub async fn seek(
  ctx: PoiseContext<'_>,
  #[description = "Specific command to show help about"]
  #[autocomplete = "poise::builtins::autocomplete_command"]
  position: String
) -> Result<(), AnyError> {
  ctx.reply("Processing...").await?;

  let player = get_player_or_fail!(ctx);

  debug!("seek: {}", position);
  let handle = player.connection.sample_provider_handle.lock().await;
  let handle = handle.as_ref().unwrap();
  let handle = handle.as_any();
  if let Some(handle) = handle.downcast_ref::<FFmpegSampleProviderHandle>() {
    let current_position = handle.get_frame_pts().unwrap();

    let position = match position.chars().nth(0).context("no first position character")? {
      '+' => current_position + Duration::from_secs(position[1..].parse::<u64>()?),
      '-' => current_position.saturating_sub(Duration::from_secs(position[1..].parse::<u64>()?)),
      _ => Duration::from_secs(position.parse::<u64>()?)
    };

    handle.seek(position).unwrap();
    player.connection.sample_buffer.clear().await;

    ctx.reply(format!("Seeked to {:?} (was: {:?})", position, current_position)).await?;
  } else {
    ctx.reply("Unsupported sample provider").await?;
  }

  Ok(())
}
