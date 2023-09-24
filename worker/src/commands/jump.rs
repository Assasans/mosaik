use anyhow::{Context, Result};
use async_trait::async_trait;
use tracing::debug;
use twilight_model::application::interaction::application_command::CommandOptionValue;
use twilight_model::application::interaction::InteractionData;
use twilight_model::gateway::payload::incoming::InteractionCreate;
use voice::VoiceConnectionState;

use super::CommandHandler;
use crate::{State, interaction_response, reply, update_reply, try_unpack, get_option_as};

pub struct JumpCommand;

#[async_trait]
impl CommandHandler for JumpCommand {
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

    debug!("jump: {}", position);
    let current_position = player.queue.position();

    // For some reason Discord sends "+5" argument as "5", but "++5" as "++5"
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

    update_reply!(state, interaction)
      .content(Some(&format!("Jumped to track {:?} (was: {:?})", position, current_position)))?
      .await?;

    Ok(())
  }
}
