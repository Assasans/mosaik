use std::path::Path;

use anyhow::{Result, Context};
use async_trait::async_trait;
use twilight_model::{gateway::payload::{incoming::InteractionCreate, outgoing::UpdateVoiceState}, application::interaction::{application_command::CommandOptionValue, InteractionData}};
use voice::VoiceConnection;

use crate::{try_unpack, State, interaction_response, get_option_as, player::Player, reply, update_reply, providers::{FileMediaProvider, MediaProvider}};
use crate::providers::SeekableHttpMediaProvider;

use super::CommandHandler;

pub struct PlayCommand;

#[async_trait]
impl CommandHandler for PlayCommand {
  async fn run(&self, state: State, interaction: Box<InteractionCreate>) -> Result<()> {
    reply!(state, interaction, &interaction_response!(
      DeferredChannelMessageWithSource,
      content("Playing...")
    )).await?;

    let command = try_unpack!(interaction.data.as_ref().context("no interaction data")?, InteractionData::ApplicationCommand)?;
    let guild_id = interaction.guild_id.unwrap();

    let source = get_option_as!(command, "source", CommandOptionValue::String)
      .map(|it| it.unwrap().clone())
      .context("no source")?; // TODO(Assasans)

    let voice_state = state.cache.voice_state(interaction.member.as_ref().unwrap().user.as_ref().unwrap().id, guild_id);
    let channel_id = get_option_as!(command, "channel", CommandOptionValue::Channel)
      .map(|it| *it.unwrap())
      .or(voice_state.map(|it| it.channel_id()))
      .unwrap();

    state.sender.command(&UpdateVoiceState::new(guild_id, channel_id, true, false))?;
    println!("connecting");

    let mut player = Player::new(state.clone(), guild_id);
    player.set_channel(channel_id);
    player.connect().await?;

    let connection = player.connection.as_ref().unwrap();

    // TODO(Assasans): Internal code
    {
      let mut ws = connection.ws.lock().await;
      ws.as_mut().unwrap().send_speaking(true).await?;
    }

    let (provider, input) = source.split_once(':').context("invalid source")?;
    let provider: Box<dyn MediaProvider> = match provider {
      "file" => Box::new(FileMediaProvider::new(Path::new(input))),
      "http_seek" => Box::new(SeekableHttpMediaProvider::new(input.to_owned())),
      _ => todo!("media provider {} is not implemented", provider)
    };

    *connection.sample_provider.lock().await = Some(provider.get_sample_provider().await?);

    let clone = connection.clone();
    tokio::spawn(async move {
      VoiceConnection::run_udp_loop(clone).await.unwrap();
    });

    state.players.write().await.insert(guild_id, player);

    let metadata = provider.get_metadata().await?;
    let metadata_string = metadata.iter()
      .map(|it| format!("`{:?}`", it))
      .collect::<Vec<String>>()
      .join("\n");

    update_reply!(state, interaction)
      .content(Some(&format!("Playing track `{:?}`\n{}", provider, metadata_string)))?
      .await?;

    Ok(())
  }
}
