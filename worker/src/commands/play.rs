use std::sync::Arc;

use anyhow::{Context, Result};
use serenity::all::ShardId;
use tracing::{error, info};
use voice::VoiceConnectionState;

use crate::player::track::Track;
use crate::player::Player;
use crate::providers::{
  FFmpegMediaProvider, MediaProvider, SberzvukMediaProvider, VkMediaProvider, YtDlpMediaProvider
};
use crate::{AnyError, PoiseContext, pretty_print_error, VOICE_MANAGER};
use crate::provider_predictor::{MediaProviderPredictor, PredictedProvider};
use crate::providers::factory::{MediaProviderFactory, YtDlpPlaylistMediaProviderFactory};

#[poise::command(prefix_command, track_edits, slash_command)]
pub async fn play(
  ctx: PoiseContext<'_>,
  #[description = "Specific command to show help about"]
  #[autocomplete = "poise::builtins::autocomplete_command"]
  source: String
) -> Result<(), AnyError> {
  ctx.reply("Processing...").await?;

  let author = ctx.author();
  let guild_id = ctx.guild_id().unwrap();

  // TODO: The fuck
  let voice_state = ctx
    .guild()
    .unwrap()
    .voice_states
    .get(&author.id)
    .map(|it| it.to_owned());
  if voice_state.is_none() {
    ctx.reply("You are not in a voice channel").await.unwrap();
  }
  let channel_id = voice_state.unwrap().channel_id;

  info!("connecting");

  let state = ctx.data();
  let mut players = state.players.write().await;
  let player = players
    .entry(guild_id)
    .or_insert_with(|| Arc::new(Player::new(state.clone(), guild_id)));

  player.set_channel(channel_id.unwrap());
  player.set_text_channel_id(ctx.channel_id());
  player.set_context(ctx.serenity_context().clone()).await;
  if !player.connection.is_connected() {
    let shard_id = ShardId(guild_id.shard_id(ctx.cache()));
    let shard_manager = ctx.framework().shard_manager();
    let shards = shard_manager.runners.lock().await;
    let shard = shards.get(&shard_id).unwrap();

    player
      .connect(VOICE_MANAGER.get().unwrap().as_ref(), ctx.cache(), &shard.runner_tx)
      .await?;
  }

  // TODO(Assasans): Internal code
  {
    let ws = player.connection.ws.read().await;
    ws.as_ref().unwrap().send_speaking(true).await?;
  }

  let predictor = MediaProviderPredictor::new();
  let splitted = source.split_once(':').and_then(|splitted| {
    if ["ffmpeg", "yt-dlp", "yt-dlp-playlist", "zvuk", "vk"].contains(&splitted.0) {
      Some(splitted)
    } else {
      None
    }
  });
  let mut providers: Vec<Box<dyn MediaProvider>> = if let Some((provider, input)) = splitted {
    match provider {
      "ffmpeg" => vec![Box::new(FFmpegMediaProvider::new(input.to_owned()))],
      "yt-dlp" => vec![Box::new(YtDlpMediaProvider::new(input.to_owned()))],
      "yt-dlp-playlist" => {
        let mut factory = YtDlpPlaylistMediaProviderFactory::new(input.to_owned());
        factory.init().await.unwrap();
        factory.get_media_providers().await.unwrap()
      },
      "zvuk" => vec![Box::new(SberzvukMediaProvider::new(input.parse::<i64>()?))],
      "vk" => {
        let (owner_id, track_id) = input.split_once('_').unwrap();
        vec![Box::new(VkMediaProvider::new(owner_id.parse::<i64>()?, track_id.parse::<i64>()?))]
      }
      _ => todo!("media provider {} is not implemented", provider)
    }
  } else {
    let prediction = predictor.predict(&source);
    info!("prediction: {:?}", prediction);

    match prediction[0].provider {
      PredictedProvider::FFmpeg => vec![Box::new(FFmpegMediaProvider::new(source))],
      PredictedProvider::YtDlp => vec![Box::new(YtDlpMediaProvider::new(source))],
      PredictedProvider::YtDlpPlaylist => {
        let mut factory = YtDlpPlaylistMediaProviderFactory::new(source);
        factory.init().await.unwrap();
        factory.get_media_providers().await.unwrap()
      }
    }
  };

  for mut provider in providers {
    match provider.init().await {
      Ok(_) => {
        let track = Track::new(provider, Some(author.id));
        let (track, position) = player.queue.push(track);

        if player.connection.state.get() != VoiceConnectionState::Playing {
          player.queue.set_position(position);
          player.play().await.unwrap();
        }

        let metadata = track.provider.get_metadata().await?;
        let metadata_string = metadata
          .iter()
          .map(|it| format!("`{:?}`", it))
          .collect::<Vec<String>>()
          .join("\n");

        ctx
          .reply(format!(
            "Added track `{:?}` to queue\n{}",
            track.provider, metadata_string
          ))
          .await
          .unwrap();
      }
      Err(error) => {
        error!("failed to init track: {:?}", error);

        ctx
          .reply(format!(
            "Failed to init provider `{:?}`:```ansi\n{}\n```",
            provider,
            pretty_print_error(error)
          ))
          .await
          .unwrap();
      }
    }
  }

  Ok(())
}
