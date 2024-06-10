use std::fmt::Display;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use poise::CreateReply;
use serenity::all::CreateEmbed;
use voice::constants::{CHANNEL_COUNT, SAMPLE_RATE};

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
    let ebur128 = player.connection.ebur128.lock().unwrap();

    fn wrap_warning(value: impl Display, is_warning: bool) -> String {
      if is_warning {
        format!("__{}__ :warning:", value)
      } else {
        format!("{}", value)
      }
    }

    let get_rms = |ms| {
      let rms = rms.calculate_rms(SAMPLE_RATE * CHANNEL_COUNT * ms / 1000);
      let rms_db = 20.0 * (rms / 1.0).log10();
      (rms, rms_db)
    };

    let rms = vec![
      (25, get_rms(25)),
      (1000, get_rms(1000)),
      (5000, get_rms(5000))
    ];
    let rms = rms.iter().map(|(window, (rms, rms_db))| {
      format!(
        "RMS over {} ms: {}",
        window,
        wrap_warning(format!("`{:.2} dBV, {:.4} Vrms`", rms_db, rms), *rms_db >= 0.0)
      )
    }).collect::<Vec<_>>().join("\n");

    let current_true_peak = 20.0 * ebur128.prev_true_peak(0).unwrap().log10();
    let true_peak = 20.0 * ebur128.true_peak(0).unwrap().log10();
    let lufs_m = ebur128.loudness_momentary().unwrap();
    let lufs_s = ebur128.loudness_shortterm().unwrap();
    let lufs_i = ebur128.loudness_global().unwrap();
    let lufs_target = -10.0;

    embed = embed.field(
      "Audio levels",
      format!(
        "{}\nCurrent: {}\nTrue Peak: {}\nMomentary loudness: {}\nShort-term loudness: {}\nIntegrated loudness: {}",
        rms,
        wrap_warning(format!("`{:.2} dBTP`", current_true_peak), current_true_peak >= 0.0),
        wrap_warning(format!("`{:.2} dBTP`", true_peak), true_peak >= 0.0),
        wrap_warning(format!("`{:.1} LUFS`", lufs_m), lufs_m > lufs_target),
        wrap_warning(format!("`{:.1} LUFS`", lufs_s), lufs_s > lufs_target),
        wrap_warning(format!("`{:.1} LUFS`", lufs_i), lufs_i > lufs_target),
      ),
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
