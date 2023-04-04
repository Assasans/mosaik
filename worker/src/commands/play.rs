use anyhow::{Result, Context};
use async_trait::async_trait;
use twilight_model::{gateway::payload::incoming::InteractionCreate, application::interaction::{application_command::{CommandData, CommandOptionValue}, InteractionData}, http::interaction::{InteractionResponse, InteractionResponseType}};
use twilight_util::builder::InteractionResponseDataBuilder;

use crate::{try_unpack, State, interaction_response, get_option_as, providers::{yt_dlp::YoutubeDlMediaProvider, MediaProvider}};

use super::CommandHandler;

pub struct PlayCommand;

#[async_trait]
impl CommandHandler for PlayCommand {
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
    let source = get_option_as!(command, "source", CommandOptionValue::String)
      .map(|it| it.unwrap().clone()) // TODO(Assasans)
      .unwrap();

    let provider = YoutubeDlMediaProvider::new(source);
    let input = provider.to_input().await?;

    if let Some(call_lock) = state.songbird.get(guild_id) {
      let mut call = call_lock.lock().await;
      let handle = call.play_input(input);

      let mut store = state.trackdata.write().await;
      store.insert(guild_id, handle);

      state
        .http
        .interaction(state.application_id)
        .update_response(&interaction.token)
        .content(Some(&format!("Playing track")))?
        .await?;
    }

    Ok(())
  }
}
