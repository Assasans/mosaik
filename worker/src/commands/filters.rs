use anyhow::{Context, Result};
use async_trait::async_trait;
use twilight_model::application::interaction::application_command::CommandOptionValue;
use twilight_model::application::interaction::InteractionData;
use twilight_model::gateway::payload::incoming::InteractionCreate;

use super::CommandHandler;
use crate::{State, interaction_response, reply, update_reply, try_unpack, get_option_as};
use crate::voice::ffmpeg::FFmpegSampleProviderHandle;

pub struct FiltersCommand;

#[async_trait]
impl CommandHandler for FiltersCommand {
  async fn run(&self, state: State, interaction: Box<InteractionCreate>) -> Result<()> {
    reply!(state, interaction, &interaction_response!(
      DeferredChannelMessageWithSource,
      content("Updating...")
    )).await?;

    let command = try_unpack!(interaction.data.as_ref().context("no interaction data")?, InteractionData::ApplicationCommand)?;
    let guild_id = interaction.guild_id.unwrap();

    let filters = get_option_as!(command, "filters", CommandOptionValue::String)
      .map(|it| it.unwrap().clone())
      .context("no filters")?; // TODO(Assasans)

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
    let player = player.lock().await;

    if let Some(connection) = &player.connection {
      {
        let handle = connection.sample_provider_handle.lock().await;
        let handle = handle.as_ref().unwrap();
        let handle = handle.as_any();
        if let Some(handle) = handle.downcast_ref::<FFmpegSampleProviderHandle>() {
          handle.init_filters(&filters);
        }
      }

      update_reply!(state, interaction)
        .content(Some("Ok"))?
        .await?;
    }

    Ok(())
  }
}
