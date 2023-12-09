use std::path::Path;
use std::sync::Arc;

use anyhow::{Result, Context};
use async_trait::async_trait;
use serenity::all::ShardId;
use tracing::info;
use voice::VoiceConnectionState;

use crate::{try_unpack, State, interaction_response, get_option_as, player::Player, reply, update_reply, providers::{MediaProvider, FFmpegMediaProvider}, PoiseContext, AnyError, VOICE_MANAGER};
use crate::player::track::Track;
use crate::providers::{SberzvukMediaProvider, VkMediaProvider, YtDlpMediaProvider};

#[poise::command(prefix_command, track_edits, slash_command)]
pub async fn play(
  ctx: PoiseContext<'_>,
  #[description = "Specific command to show help about"]
  #[autocomplete = "poise::builtins::autocomplete_command"]
  source: String
) -> Result<(), AnyError> {
  let author = ctx.author();
  let guild_id = ctx.guild_id().unwrap();

  // TODO: The fuck
  let voice_state = ctx.guild().unwrap().voice_states.get(&author.id).map(|it| it.to_owned());
  if voice_state.is_none() {
    ctx.reply("You are not in a voice channel").await.unwrap();
  }
  let channel_id = voice_state.unwrap().channel_id;

  info!("connecting");

  let state = ctx.data();
  let mut players = state.players.write().await;
  let player = players.entry(guild_id).or_insert_with(|| Arc::new(Player::new(state.clone(), guild_id)));

  player.set_channel(channel_id.unwrap());
  if !player.connection.is_connected() {
    let shard_id = ShardId(guild_id.shard_id(ctx.cache()));
    let shard_manager = ctx.framework().shard_manager();
    let shards = shard_manager.runners.lock().await;
    let shard = shards.get(&shard_id).unwrap();

    player.connect(VOICE_MANAGER.get().unwrap().as_ref(), ctx.cache(), &shard.runner_tx).await?;
  }

  // TODO(Assasans): Internal code
  {
    let ws = player.connection.ws.read().await;
    ws.as_ref().unwrap().send_speaking(true).await?;
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

  let track = Track::new(provider, Some(author.id));
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

  ctx.reply(format!("Added track `{:?}` to queue\n{}", track.provider, metadata_string)).await.unwrap();
  Ok(())
}
