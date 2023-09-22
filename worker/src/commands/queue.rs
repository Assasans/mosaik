use std::fmt::Write;
use std::sync::atomic::Ordering;
use std::time::Duration;
use anyhow::Result;
use async_trait::async_trait;
use twilight_model::gateway::payload::incoming::InteractionCreate;

use super::CommandHandler;
use crate::{State, interaction_response, reply, update_reply};
use crate::providers::{get_metadata, MediaMetadata};
use crate::voice::ffmpeg::FFmpegSampleProviderHandle;

pub struct QueueCommand;

#[async_trait]
impl CommandHandler for QueueCommand {
  async fn run(&self, state: State, interaction: &InteractionCreate) -> Result<()> {
    reply!(state, interaction, &interaction_response!(
      DeferredChannelMessageWithSource,
      content("Pausing...")
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

    let mut fmt = String::new();
    let mut index = 0;

    let handle = player.connection.sample_provider_handle.lock().await;
    let handle = handle.as_ref().unwrap();
    let handle = handle.as_any();
    if let Some(handle) = handle.downcast_ref::<FFmpegSampleProviderHandle>() {
      // TODO(Assasans): Make get_frame_pts return raw PTS (samples count)?
      let mut pts = handle.get_frame_pts().unwrap();
      let buffer_length = player.connection.jitter_buffer_size.load(Ordering::Relaxed) * 1000 / 2 / 48000;
      let buffer_length = Duration::from_millis(buffer_length as u64);
      pts -= buffer_length;

      fmt.write_fmt(format_args!("pts: {:?} (buffer {:?})\n\n", pts, buffer_length)).unwrap();
    }

    let tracks = {
      let tracks = player.queue.tracks.read().unwrap();
      tracks.iter().map(|it| it.clone()).collect::<Vec<_>>()
    };
    for track in &tracks {
      let metadata = track.provider.get_metadata().await.unwrap();
      let title = get_metadata!(metadata, MediaMetadata::Title(id) => id.as_str())
        .unwrap_or("**provider not supported**");
      let duration = get_metadata!(metadata, MediaMetadata::Duration(duration) => duration)
        .map(|duration| format!(" [{:?}]", duration))
        .unwrap_or(String::new());
      let is_current = index == player.queue.position();

      fmt.write_fmt(format_args!(
        "{}. {}{}{}\n",
        index + 1,
        if is_current { ":arrow_forward: " } else { "" },
        title,
        duration
      )).unwrap();
      index += 1;
    }

    update_reply!(state, interaction)
      .content(Some(&fmt))?
      .await?;

    Ok(())
  }
}
