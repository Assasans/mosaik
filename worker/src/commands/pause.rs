use anyhow::Result;
use async_trait::async_trait;
use twilight_model::gateway::payload::incoming::InteractionCreate;

use super::CommandHandler;
use crate::{State, interaction_response, reply, update_reply};

pub struct PauseCommand;

#[async_trait]
impl CommandHandler for PauseCommand {
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

    player.connection.set_paused(!player.connection.is_paused())?;

    update_reply!(state, interaction)
      .content(Some("Ok"))?
      .await?;

    Ok(())
  }
}
