pub mod util;
pub mod commands;
pub mod providers;

use anyhow::Context;
use commands::{JoinCommand, CommandHandler, PlayCommand};
use futures::StreamExt;
use songbird::{
  input::{Input, LiveInput, AudioStream, HttpRequest},
  tracks::{PlayMode, TrackHandle},
  Songbird,
};
use reqwest::Client;
use symphonia::core::io::{MediaSource, ReadOnlySource};
use tracing_subscriber::layer::SubscriberExt;
use twilight_cache_inmemory::InMemoryCache;
use twilight_util::builder::{command::{StringBuilder, ChannelBuilder}, InteractionResponseDataBuilder};

use std::{collections::HashMap, env, error::Error, future::Future, sync::Arc};
use tokio::{sync::RwLock, fs::File, io::AsyncReadExt};
use twilight_gateway::{Cluster, Shard, Event, Intents};
use twilight_http::{Client as HttpClient, client::InteractionClient};
use twilight_model::{
  channel::{Message, ChannelType, message::MessageFlags},
  gateway::payload::incoming::MessageCreate,
  id::{marker::{GuildMarker, ChannelMarker, ApplicationMarker}, Id}, application::{command::CommandOption, interaction::InteractionData}, http::interaction::{InteractionResponse, InteractionResponseType},
};
use twilight_standby::Standby;

use crate::providers::{yt_dlp::YoutubeDlMediaProvider, MediaProvider};

pub type State = Arc<StateRef>;

#[derive(Debug)]
pub struct StateRef {
  http: HttpClient,
  cache: InMemoryCache,
  application_id: Id<ApplicationMarker>,
  trackdata: RwLock<HashMap<Id<GuildMarker>, TrackHandle>>,
  songbird: Songbird,
  standby: Standby,
}

fn spawn(
  fut: impl Future<Output = Result<(), Box<dyn Error + Send + Sync + 'static>>> + Send + 'static,
) {
  tokio::spawn(async move {
    if let Err(why) = fut.await {
      tracing::debug!("handler error: {:?}", why);
    }
  });
}

macro_rules! localizations {
  ($($key:expr => $value:expr),*) => {{
    let mut map = ::std::collections::HashMap::new();
    $(map.insert($key.to_owned(), $value.to_owned());)*
    map
  }};
}

macro_rules! argument {
  ($type:ident, $name:expr, $description:expr $(, $method:ident ( $( $arg:expr ),* ))*) => {{
    let mut builder = $type::new($name, $description);
    $(builder = builder.$method($($arg),*);)*
    builder.build()
  }};
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
  tracing_subscriber::fmt()
    .with_max_level(tracing::Level::DEBUG)
    .init();

  // tracing::subscriber::set_global_default(
  //   tracing_subscriber::registry().with(tracing_tracy::TracyLayer::new())
  // ).expect("set up the subscriber");

  let (mut events, state) = {
    let token = env::var("DISCORD_TOKEN")?;

    let http = HttpClient::new(token.clone());
    let cache = InMemoryCache::new();
    let user_id = http.current_user().await?.model().await?.id;
    let application_id = http.current_user_application().await?.model().await?.id;
    let interactions = http.interaction(application_id);

    interactions
      .create_guild_command(Id::<GuildMarker>::new(646393082430095383))
      .chat_input("join", "Join channel")?
      .description_localizations(&localizations! {
        "ru" => "Зайти в канал"
      })?
      .command_options(&[
        argument!(
          ChannelBuilder,
          "channel",
          "Channel to join",
          required(false),
          description_localizations(&localizations! {
            "ru" => "Канал для входа"
          }),
          channel_types([ChannelType::GuildVoice, ChannelType::GuildStageVoice])
        )
      ])?
      .await?;

    interactions
      .create_guild_command(Id::<GuildMarker>::new(646393082430095383))
      .chat_input("play", "Play a track")?
      .description_localizations(&localizations! {
        "ru" => "Включить трек"
      })?
      .command_options(&[
        argument!(
          StringBuilder,
          "source",
          "Search query or URL",
          required(true),
          description_localizations(&localizations! {
            "ru" => "Поисковый запрос или ссылка"
          })
        )
      ])?
      .await?;

    let intents = Intents::all();
    let (cluster, events) = Cluster::new(token, intents).await?;
    cluster.up().await;

    let songbird = Songbird::twilight(Arc::new(cluster), user_id);

    (
      events,
      Arc::new(StateRef {
        http,
        cache,
        application_id,
        trackdata: Default::default(),
        songbird,
        standby: Standby::new(),
      })
    )
  };

  let handlers: &mut HashMap<&'static str, Box<dyn CommandHandler>> = Box::leak(Box::new(HashMap::from([ // TODO(Assasans): Memory leak
    ("join", Box::new(JoinCommand {}) as Box<dyn CommandHandler>),
    ("play", Box::new(PlayCommand {}) as Box<dyn CommandHandler>)
  ])));

  while let Some((shard_id, event)) = events.next().await {
    state.standby.process(&event);
    state.songbird.process(&event).await;
    state.cache.update(&event);

    if let Event::InteractionCreate(interaction) = event {
      let command = try_unpack!(interaction.data.as_ref().context("no interaction data")?, InteractionData::ApplicationCommand)?;

      match handlers.get(command.name.as_str()) {
        Some(handler) => {
          let cloned = state.clone();
          tokio::spawn(async move {
            let result = handler.run(cloned, interaction).await;
            if let Err(error) = result {
              tracing::debug!("handler error: {:?}", error);
            }
          });
        },
        None => {
          state
            .http
            .interaction(state.application_id)
            .create_response(interaction.id, &interaction.token, &InteractionResponse {
              kind: InteractionResponseType::ChannelMessageWithSource,
              data: InteractionResponseDataBuilder::new()
                .content(format!("Unknown handler for command `{}`", command.name.as_str()))
                // .flags(MessageFlags::EPHEMERAL)
                .build()
                .into()
            })
            .await?;
        }
      }
    } else if let Event::MessageCreate(msg) = event {
      if msg.guild_id.is_none() || !msg.content.starts_with('!') {
        continue;
      }

      match msg.content.splitn(2, ' ').next() {
        Some("!leave") => spawn(leave(msg.0, Arc::clone(&state))),
        Some("!pause") => spawn(pause(msg.0, Arc::clone(&state))),
        Some("!seek") => spawn(seek(msg.0, Arc::clone(&state))),
        Some("!stop") => spawn(stop(msg.0, Arc::clone(&state))),
        Some("!volume") => spawn(volume(msg.0, Arc::clone(&state))),
        _ => continue,
      }
    }
  }

  Ok(())
}

async fn leave(msg: Message, state: State) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
  tracing::debug!(
    "leave command in channel {} by {}",
    msg.channel_id,
    msg.author.name
  );

  let guild_id = msg.guild_id.unwrap();
  state.songbird.leave(guild_id).await?;

  state
    .http
    .create_message(msg.channel_id)
    .content("Left the channel")?
    .await?;

  Ok(())
}

async fn pause(msg: Message, state: State) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
  tracing::debug!(
    "pause command in channel {} by {}",
    msg.channel_id,
    msg.author.name
  );

  let guild_id = msg.guild_id.unwrap();

  let store = state.trackdata.read().await;

  let content = if let Some(handle) = store.get(&guild_id) {
    let info = handle.get_info().await?;

    let paused = match info.playing {
      PlayMode::Play => {
        let _success = handle.pause();
        false
      }
      _ => {
        let _success = handle.play();
        true
      }
    };

    let action = if paused { "Unpaused" } else { "Paused" };

    format!("{} the track", action)
  } else {
    format!("No track to (un)pause!")
  };

  state
    .http
    .create_message(msg.channel_id)
    .content(&content)?
    .await?;

  Ok(())
}

async fn seek(msg: Message, state: State) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
  tracing::debug!(
    "seek command in channel {} by {}",
    msg.channel_id,
    msg.author.name
  );
  state
    .http
    .create_message(msg.channel_id)
    .content("Where in the track do you want to seek to (in seconds)?")?
    .await?;

  let author_id = msg.author.id;
  let msg = state
    .standby
    .wait_for_message(msg.channel_id, move |new_msg: &MessageCreate| {
      new_msg.author.id == author_id
    })
    .await?;
  let guild_id = msg.guild_id.unwrap();
  let position = msg.content.parse::<u64>()?;

  let store = state.trackdata.read().await;

  let content = if let Some(handle) = store.get(&guild_id) {
    let _success = handle.seek(std::time::Duration::from_secs(position));
    format!("Seeked to {}s", position)
  } else {
    format!("No track to seek over!")
  };

  state
    .http
    .create_message(msg.channel_id)
    .content(&content)?
    .await?;

  Ok(())
}

async fn stop(msg: Message, state: State) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
  tracing::debug!(
    "stop command in channel {} by {}",
    msg.channel_id,
    msg.author.name
  );

  let guild_id = msg.guild_id.unwrap();

  if let Some(call_lock) = state.songbird.get(guild_id) {
    let mut call = call_lock.lock().await;
    let _ = call.stop();
  }

  state
    .http
    .create_message(msg.channel_id)
    .content("Stopped the track")?
    .await?;

  Ok(())
}

async fn volume(msg: Message, state: State) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
  tracing::debug!(
    "volume command in channel {} by {}",
    msg.channel_id,
    msg.author.name
  );
  state
    .http
    .create_message(msg.channel_id)
    .content("What's the volume you want to set (0.0-10.0, 1.0 being the default)?")?
    .await?;

  let author_id = msg.author.id;
  let msg = state
    .standby
    .wait_for_message(msg.channel_id, move |new_msg: &MessageCreate| {
      new_msg.author.id == author_id
    })
    .await?;
  let guild_id = msg.guild_id.unwrap();
  let volume = msg.content.parse::<f64>()?;

  if !volume.is_finite() || volume > 10.0 || volume < 0.0 {
    state
      .http
      .create_message(msg.channel_id)
      .content("Invalid volume!")?
      .await?;

    return Ok(());
  }

  let store = state.trackdata.read().await;

  let content = if let Some(handle) = store.get(&guild_id) {
    let _success = handle.set_volume(volume as f32);
    format!("Set the volume to {}", volume)
  } else {
    format!("No track to change volume!")
  };

  state
    .http
    .create_message(msg.channel_id)
    .content(&content)?
    .await?;

  Ok(())
}
