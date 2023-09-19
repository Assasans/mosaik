use std::path::Path;
use std::sync::Arc;

use anyhow::{Result, Context};
use async_trait::async_trait;
use twilight_model::{gateway::payload::{incoming::InteractionCreate, outgoing::UpdateVoiceState}, application::interaction::{application_command::CommandOptionValue, InteractionData}};
use voice::VoiceConnectionState;

use crate::{try_unpack, State, interaction_response, get_option_as, player::Player, reply, update_reply, providers::{MediaProvider, FFmpegMediaProvider}};
use crate::player::track::Track;
use crate::providers::{SberzvukMediaProvider, VkMediaProvider, YtDlpMediaProvider};

use super::CommandHandler;

pub struct PlayCommand;

#[async_trait]
impl CommandHandler for PlayCommand {
  async fn run(&self, state: State, interaction: &InteractionCreate) -> Result<()> {
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
    let channel_id = match get_option_as!(command, "channel", CommandOptionValue::Channel)
      .map(|it| *it.unwrap())
      .or(voice_state.map(|it| it.channel_id())) {
      Some(value) => value,
      None => {
        update_reply!(state, interaction)
          .content(Some("You are not in a voice channel"))?
          .await?;
        return Ok(());
      }
    };

    state.sender.command(&UpdateVoiceState::new(guild_id, channel_id, true, false))?;
    println!("connecting");

    let mut players = state.players.write().await;
    let player = players.get(&guild_id);
    let player = if let Some(player) = player {
      player.clone()
    } else {
      let player = Arc::new(Player::new(state.clone(), guild_id));
      players.insert(guild_id, player.clone());
      player
    };

    player.set_channel(channel_id);
    if !player.connection.is_connected() {
      player.connect().await?;
    }

    // TODO(Assasans): Internal code
    {
      let mut ws = player.connection.ws.lock().await;
      ws.as_mut().unwrap().send_speaking(true).await?;
    }

    let (provider, input) = source.split_once(':').context("invalid source")?;
    let mut provider: Box<dyn MediaProvider> = match provider {
      "ffmpeg" => Box::new(FFmpegMediaProvider::new(input.to_owned())),
      "yt-dlp" => Box::new(YtDlpMediaProvider::new(input.to_owned())),
      "zvuk" => Box::new(SberzvukMediaProvider::new(input.parse::<i64>()?)),
      "vk" => {
        let (owner_id, track_id) = input.split_once('_').unwrap();
        Box::new(VkMediaProvider::new(
          owner_id.parse::<i64>()?,
          track_id.parse::<i64>()?
        ))
      },
      _ => todo!("media provider {} is not implemented", provider)
    };
    provider.init().await?;

    let track = Track::new(provider, interaction.user.as_ref().map(|user| user.id));
    let (track, position) = player.queue.push(track);

    if player.connection.state.get() != VoiceConnectionState::Playing {
      player.queue.set_position(position);
      player.play().await.unwrap();
    }

    let metadata = track.provider.get_metadata().await?;
    let metadata_string = metadata.iter()
      .map(|it| format!("`{:?}`", it))
      .collect::<Vec<String>>()
      .join("\n");

    update_reply!(state, interaction)
      .content(Some(&format!("Added track `{:?}` to queue\n{}", track.provider, metadata_string)))?
      .await?;

    Ok(())
  }
}
