use anyhow::{Result, Context};
use async_trait::async_trait;
use twilight_model::{gateway::payload::incoming::InteractionCreate, application::interaction::{application_command::{CommandData, CommandOptionValue}, InteractionData}, http::interaction::{InteractionResponse, InteractionResponseType}};
use twilight_util::builder::InteractionResponseDataBuilder;

use crate::{try_unpack, State, interaction_response, get_option_as};

use super::CommandHandler;

pub struct JoinCommand;

#[async_trait]
impl CommandHandler for JoinCommand {
  async fn run(&self, state: State, interaction: Box<InteractionCreate>) -> Result<()> {
    state
      .http
      .interaction(state.application_id)
      .create_response(interaction.id, &interaction.token, &interaction_response!(
        DeferredChannelMessageWithSource,
        content("Joining...")
      ))
      .await?;

    let command = try_unpack!(interaction.data.as_ref().context("no interaction data")?, InteractionData::ApplicationCommand)?;
    let guild_id = interaction.guild_id.unwrap();
    let voice_state = state.cache.voice_state(interaction.member.as_ref().unwrap().user.as_ref().unwrap().id, guild_id).unwrap();
    let channel_id = get_option_as!(command, "channel", CommandOptionValue::Channel)
      .map(|it| *it.unwrap())
      .or(Some(voice_state.channel_id()))
      .unwrap();

    state.songbird.join(guild_id, channel_id).await?;

    state
      .http
      .interaction(state.application_id)
      .update_response(&interaction.token)
      .content(Some(&format!("Joined channel <#{}>", channel_id)))?
      .await?;

    Ok(())
  }
}
