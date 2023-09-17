use std::path::Path;
use std::sync::Arc;

use anyhow::{Result, Context};
use async_trait::async_trait;
use tokio::sync::Mutex;
use twilight_model::{gateway::payload::{incoming::InteractionCreate, outgoing::UpdateVoiceState}, application::interaction::{application_command::CommandOptionValue, InteractionData}};
use voice::VoiceConnection;

use crate::{try_unpack, State, interaction_response, get_option_as, player::Player, reply, update_reply, providers::{MediaProvider, FFmpegMediaProvider}};
use crate::providers::{SberzvukMediaProvider, YtDlpMediaProvider};

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

    let mut players = state.players.write().await;
    let player = players.get(&guild_id);
    let player = if let Some(player) = player {
      player.clone()
    } else {
      let player = Arc::new(Mutex::new(Player::new(state.clone(), guild_id)));
      players.insert(guild_id, player.clone());
      player
    };
    let mut player = player.lock().await;

    player.set_channel(channel_id);
    if player.connection.is_none() {
      player.connect().await?;
    }

    let connection = player.connection.as_ref().unwrap();

    // TODO(Assasans): Internal code
    {
      let mut ws = connection.ws.lock().await;
      ws.as_mut().unwrap().send_speaking(true).await?;
    }

    let (provider, input) = source.split_once(':').context("invalid source")?;
    let mut provider: Box<dyn MediaProvider> = match provider {
      "ffmpeg" => Box::new(FFmpegMediaProvider::new(input.to_owned())),
      "yt-dlp" => Box::new(YtDlpMediaProvider::new(input.to_owned())),
      "zvuk" => Box::new(SberzvukMediaProvider::new(input.parse::<i64>()?)),
      _ => todo!("media provider {} is not implemented", provider)
    };
    provider.init().await?;

    let sample_provider = provider.get_sample_provider().await?;
    *connection.sample_provider_handle.lock().await = Some(sample_provider.get_handle());
    *connection.sample_provider.lock().await = Some(sample_provider);

    let clone = connection.clone();
    tokio::spawn(async move {
      VoiceConnection::run_udp_loop(clone).await.unwrap();
    });

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
