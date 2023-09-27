use std::time::Duration;
use anyhow::Result;
use async_trait::async_trait;
use twilight_model::gateway::payload::incoming::InteractionCreate;
use twilight_util::builder::embed::{EmbedBuilder, EmbedFieldBuilder};

use super::CommandHandler;
use crate::{State, interaction_response, reply, update_reply};
use crate::voice::ffmpeg::FFmpegSampleProviderHandle;

pub struct DebugCommand;

#[async_trait]
impl CommandHandler for DebugCommand {
  async fn run(&self, state: State, interaction: &InteractionCreate) -> Result<()> {
    reply!(state, interaction, &interaction_response!(
      DeferredChannelMessageWithSource,
      content("Processing...")
    )).await?;

    // let command = try_unpack!(interaction.data.as_ref().context("no interaction data")?, InteractionData::ApplicationCommand)?;
    let guild_id = interaction.guild_id.unwrap();

    let players = state.players.read().await;
    let player = players.get(&guild_id);
    let player = if let Some(player) = player {
      player
    } else {
      update_reply!(state, interaction)
        .content(Some("No player"))?
        .await?;
      return Ok(());
    };

    let mut embed = EmbedBuilder::new()
      .title("Debug information");

    {
      let track = player.queue.get_current().upgrade().unwrap();
      embed = embed.field(EmbedFieldBuilder::new("Track", format!(
        "provider: `{:?}`\ncreator: `{:?}`",
        track.provider,
        track.creator
      )));
    }

    let handle = player.connection.sample_provider_handle.lock().await;
    let handle = handle.as_ref().unwrap();
    let handle = handle.as_any();
    if let Some(handle) = handle.downcast_ref::<FFmpegSampleProviderHandle>() {
      // TODO(Assasans): Make get_frame_pts return raw PTS (samples count)?
      let decoder_pts = handle.get_frame_pts().unwrap();
      let buffer_length = player.connection.sample_buffer.len() * 1000 / 2 / 48000;
      let buffer_length = Duration::from_millis(buffer_length as u64);
      let pts = decoder_pts - buffer_length;

      embed = embed.field(EmbedFieldBuilder::new("Decoder", format!(
        "pts: `{:?}` (decoder: `{:?}`, buffered: `{:?}`)",
        pts,
        decoder_pts,
        buffer_length
      )));
    }

    embed = embed.field(EmbedFieldBuilder::new("Queue", format!(
      "tracks: `{}`\nmode: `{:?}`",
      player.queue.len(),
      player.queue.mode.read().unwrap()
    )));

    let ws = player.connection.ws.read().await;
    if let Some(ws) = ws.as_ref() {
      if let Some(ready) = &ws.ready {
        embed = embed.field(EmbedFieldBuilder::new("WebSocketVoiceConnection", format!(
          "ssrc: `{}`\nendpoint: `{}:{}`",
          ready.ssrc,
          ready.ip,
          ready.port
        )).inline());
      }
    }

    let udp = player.connection.udp.lock().await;
    if let Some(udp) = udp.as_ref() {
      embed = embed.field(EmbedFieldBuilder::new("UdpVoiceConnection", format!(
        "sequence: `{}`\ntimestamp: `{}`",
        udp.sequence.0.0,
        udp.timestamp.0.0
      )).inline());
    }

    let embed = embed.validate()?
      .build();

    update_reply!(state, interaction)
      .embeds(Some(&[embed]))?
      .await?;

    Ok(())
  }
}
