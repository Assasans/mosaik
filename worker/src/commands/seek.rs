use std::sync::atomic::Ordering;
use std::time::Duration;
use anyhow::{Context, Result};
use async_trait::async_trait;
use tracing::debug;
use twilight_model::application::interaction::application_command::CommandOptionValue;
use twilight_model::application::interaction::InteractionData;
use twilight_model::gateway::payload::incoming::InteractionCreate;

use super::CommandHandler;
use crate::{State, interaction_response, reply, update_reply, try_unpack, get_option_as};
use crate::voice::ffmpeg::FFmpegSampleProviderHandle;

pub struct SeekCommand;

#[async_trait]
impl CommandHandler for SeekCommand {
  async fn run(&self, state: State, interaction: &InteractionCreate) -> Result<()> {
    reply!(state, interaction, &interaction_response!(
      DeferredChannelMessageWithSource,
      content("Processing...")
    )).await?;

    let command = try_unpack!(interaction.data.as_ref().context("no interaction data")?, InteractionData::ApplicationCommand)?;
    let guild_id = interaction.guild_id.unwrap();

    let position = get_option_as!(command, "position", CommandOptionValue::String)
      .map(|it| it.unwrap().clone())
      .context("no position")?; // TODO(Assasans)

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

    debug!("seek: {}", position);
    let handle = player.connection.sample_provider_handle.lock().await;
    let handle = handle.as_ref().unwrap();
    let handle = handle.as_any();
    if let Some(handle) = handle.downcast_ref::<FFmpegSampleProviderHandle>() {
      let current_position = handle.get_frame_pts().unwrap();

      // For some reason Discord sends "+5" argument as "5", but "++5" as "++5"
      let position = match position.chars().nth(0).context("no first position character")? {
        '+' => current_position + Duration::from_secs(position[1..].parse::<u64>()?),
        '-' => current_position.saturating_sub(Duration::from_secs(position[1..].parse::<u64>()?)),
        _ => Duration::from_secs(position.parse::<u64>()?)
      };

      handle.seek(position).unwrap();
      player.connection.jitter_buffer_reset.store(true, Ordering::Relaxed);

      update_reply!(state, interaction)
        .content(Some(&format!("Seeked to {:?} (was: {:?})", position, current_position)))?
        .await?;
    } else {
      update_reply!(state, interaction)
        .content(Some("Unsupported sample provider"))?
        .await?;
    }

    Ok(())
  }
}
