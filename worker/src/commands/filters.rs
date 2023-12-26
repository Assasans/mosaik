use anyhow::Result;
use decoder::Decoder;
use tracing::error;

use crate::state::get_player_or_fail;
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

  let player = get_player_or_fail!(ctx);

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
          let description = Decoder::error_code_to_string(error);
          error!("failed to init filters: {:?} ({})", error, description);
          ctx
            .reply(format!("Failed to set filter graph: `{:?} ({})`", error, description))
            .await?;
        }
      }
    }
  } else {
    ctx.reply("Unsupported sample provider").await?;
  }

  Ok(())
}
