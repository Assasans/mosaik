use anyhow::Result;
use tracing::error;

use crate::voice::ffmpeg::FFmpegSampleProviderHandle;
use crate::{AnyError, PoiseContext};

#[poise::command(prefix_command, track_edits, slash_command)]
pub async fn filters(
  ctx: PoiseContext<'_>,
  #[description = "Specific command to show help about"]
  #[autocomplete = "poise::builtins::autocomplete_command"]
  filters: String
) -> Result<(), AnyError> {
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

  let handle = player.connection.sample_provider_handle.lock().await;
  let handle = handle.as_ref().unwrap();
  let handle = handle.as_any();
  if let Some(handle) = handle.downcast_ref::<FFmpegSampleProviderHandle>() {
    if filters == "bypass" {
      handle.set_enable_filter_graph(false).unwrap();
      ctx.reply("Disabled filter graph").await?;
    } else {
      match handle.init_filters(&filters) {
        Ok(()) => {
          handle.set_enable_filter_graph(true).unwrap();
          ctx.reply(format!("Set filter graph: `{}`", filters)).await?;
        }
        Err(error) => {
          error!("failed to init filters: {:?}", error);
          ctx.reply(format!("Failed to set filter graph: `{:?}`", error)).await?;
        }
      }
    }
  } else {
    ctx.reply("Unsupported sample provider").await?;
  }

  Ok(())
}
