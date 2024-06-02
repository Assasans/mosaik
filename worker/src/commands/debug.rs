use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use poise::CreateReply;
use serenity::all::CreateEmbed;

use crate::{AnyError, PoiseContext};
use crate::player::Player;
use crate::state::get_player_or_fail;
use crate::voice::ffmpeg::FFmpegSampleProviderHandle;

#[poise::command(prefix_command, track_edits, slash_command)]
pub async fn debug(ctx: PoiseContext<'_>) -> Result<(), AnyError> {
  ctx.reply("Processing...").await?;

  let player: Arc<Player> = get_player_or_fail!(ctx);

  let mut embed = CreateEmbed::default().title("Debug information");

  let track = player.queue.get_current().upgrade().unwrap();
  embed = embed.field(
    "Track",
    format!("provider: `{:?}`\ncreator: `{:?}`", track.provider, track.creator),
    false
  );

  {
    let handle = player.connection.sample_provider_handle.lock().await;
    let handle = handle.as_ref().unwrap();
    let handle = handle.as_any();
    if let Some(handle) = handle.downcast_ref::<FFmpegSampleProviderHandle>() {
      // TODO(Assasans): Make get_frame_pts return raw PTS (samples count)?
      let decoder_pts = handle.get_frame_pts().unwrap();
      let buffer_length = player.connection.sample_buffer.len() * 1000 / 2 / 48000;
      let buffer_length = Duration::from_millis(buffer_length as u64);
      let pts = decoder_pts - buffer_length;

      embed = embed.field(
        "Decoder",
        format!(
          "pts: `{:?}` (decoder: `{:?}`, buffered: `{:?}`)",
          pts, decoder_pts, buffer_length
        ),
        false
      );
    }
  }

  {
    let rms = player.connection.rms.lock().unwrap();
    let rms = rms.calculate_rms();
    let rms_db = 20.0 * (rms / 1.0).log10();
    embed = embed.field(
      "Audio levels",
      format!("RMS over 25 ms: `{} dBV, {} Vrms`", rms_db, rms),
      true
    );
  }

  embed = embed.field(
    "Queue",
    format!(
      "tracks: `{}`\nmode: `{:?}`",
      player.queue.len(),
      player.queue.mode.read().unwrap()
    ),
    false
  );

  {
    let ws = player.connection.ws.read().await;
    if let Some(ws) = ws.as_ref() {
      if let Some(ready) = &ws.ready {
        embed = embed.field(
          "WebSocketVoiceConnection",
          format!("ssrc: `{}`\nendpoint: `{}:{}`", ready.ssrc, ready.ip, ready.port),
          true
        );
      }
    }
  }

  {
    let udp = player.connection.udp.lock().await;
    if let Some(udp) = udp.as_ref() {
      embed = embed.field(
        "UdpVoiceConnection",
        format!("sequence: `{}`\ntimestamp: `{}`", udp.sequence.0 .0, udp.timestamp.0 .0),
        true
      );
    }
  }

  ctx.send(ctx.reply_builder(CreateReply::default().embed(embed))).await?;

  Ok(())
}
