use anyhow::{Context, Result};
use async_trait::async_trait;
use tracing::error;
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
          if filters == "bypass" {
            handle.set_enable_filter_graph(false).unwrap();
          } else {
            handle.set_enable_filter_graph(true).unwrap();
            match handle.init_filters(&filters) {
              Ok(()) => {},
              Err(error) => {
                error!("failed to init filters: {:?}", error);

                // TODO(Assasans): UDP loop may call get_samples after init_filters failed,
                // but filter graph is still not disabled, causing segfault.
                handle.set_enable_filter_graph(false).unwrap();
              }
            }
          }
        }
      }

      if filters == "bypass" {
        update_reply!(state, interaction)
          .content(Some("Disabled filter graph"))?
          .await?;
      } else {
        update_reply!(state, interaction)
          .content(Some(&format!("Set filter graph: `{}`", filters)))?
          .await?;
      }

      // update_reply!(state, interaction)
      //   .content(Some("Error"))?
      //   .await?;
    }

    Ok(())
  }
}
