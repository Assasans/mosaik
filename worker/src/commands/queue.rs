use std::fmt::Write;
use std::time::Duration;

use anyhow::Result;

use crate::providers::{get_metadata, MediaMetadata};
use crate::state::get_player_or_fail;
use crate::voice::ffmpeg::FFmpegSampleProviderHandle;
use crate::{AnyError, PoiseContext};

#[poise::command(prefix_command, track_edits, slash_command)]
pub async fn queue(ctx: PoiseContext<'_>) -> Result<(), AnyError> {
  ctx.reply("Processing...").await?;

  let player = get_player_or_fail!(ctx);
  let mut fmt = String::new();
  let mut index = 0;

  let handle = player.connection.sample_provider_handle.lock().await;
  let handle = handle.as_ref().unwrap();
  let handle = handle.as_any();
  if let Some(handle) = handle.downcast_ref::<FFmpegSampleProviderHandle>() {
    // TODO(Assasans): Make get_frame_pts return raw PTS (samples count)?
    let mut pts = handle.get_frame_pts().unwrap();
    let buffer_length = player.connection.sample_buffer.len() * 1000 / 2 / 48000;
    let buffer_length = Duration::from_millis(buffer_length as u64);
    pts -= buffer_length;

    fmt
      .write_fmt(format_args!("pts: {:?} (buffer {:?})\n\n", pts, buffer_length))
      .unwrap();
  }

  let tracks = {
    let tracks = player.queue.tracks.read().unwrap();
    tracks.iter().map(|it| it.clone()).collect::<Vec<_>>()
  };
  for track in &tracks {
    let metadata = track.provider.get_metadata().await.unwrap();
    let title =
      get_metadata!(metadata, MediaMetadata::Title(id) => id.as_str()).unwrap_or("**provider not supported**");
    let duration = get_metadata!(metadata, MediaMetadata::Duration(duration) => duration)
      .map(|duration| format!(" [{:?}]", duration))
      .unwrap_or(String::new());
    let is_current = index == player.queue.position();

    fmt
      .write_fmt(format_args!(
        "{}. {}{}{}\n",
        index + 1,
        if is_current { ":arrow_forward: " } else { "" },
        title,
        duration
      ))
      .unwrap();
    index += 1;
  }
  ctx.reply(fmt).await?;

  Ok(())
}
