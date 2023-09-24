pub mod util;
pub mod commands;
pub mod providers;
pub mod voice;
pub mod player;

use anyhow::Context;
use commands::{CommandHandler, PlayCommand, PauseCommand, FiltersCommand, QueueCommand, DebugCommand, SeekCommand, JumpCommand};
use player::Player;
use tracing_subscriber::{layer::SubscriberExt, EnvFilter, util::SubscriberInitExt};
use twilight_cache_inmemory::InMemoryCache;
use twilight_util::builder::{command::StringBuilder, InteractionResponseDataBuilder};

use std::{collections::HashMap, env, error::Error, future::Future, sync::Arc};
use std::fmt::{Debug, Write};
use regex::Regex;
use tokio::sync::{Mutex, RwLock};
use twilight_gateway::{Shard, Event, Intents, ShardId, MessageSender};
use twilight_http::Client as HttpClient;
use twilight_model::{
  id::{marker::{GuildMarker, ApplicationMarker}, Id}, application::{interaction::InteractionData}, http::interaction::{InteractionResponse, InteractionResponseType}, gateway::payload::outgoing::UpdateVoiceState,
};
use twilight_standby::Standby;

pub type State = Arc<StateRef>;

pub struct StateRef {
  sender: MessageSender,
  http: HttpClient,
  cache: InMemoryCache,
  application_id: Id<ApplicationMarker>,
  players: RwLock<HashMap<Id<GuildMarker>, Arc<Player>>>,
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
  if env::var("MOSAIK_DEBUG_TRACY").map_or(false, |it| it == "1") {
    tracing_subscriber::registry()
      .with(tracing_tracy::TracyLayer::new())
      .with(tracing_subscriber::fmt::Layer::new())
      .init();
  } else {
    tracing_subscriber::fmt()
      .with_max_level(tracing::Level::DEBUG)
      .with_env_filter(EnvFilter::from_default_env())
      .init();
  }

  let guild_id = Id::<GuildMarker>::new(env::var("DISCORD_TEST_GUILD")?.parse()?);
  let (mut shard, state) = {
    let token = env::var("DISCORD_TOKEN")?;

    let http = HttpClient::new(token.clone());
    let cache = InMemoryCache::new();
    let user_id = http.current_user().await?.model().await?.id;
    let application_id = http.current_user_application().await?.model().await?.id;
    let interactions = http.interaction(application_id);

    interactions
      .create_guild_command(guild_id)
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

    interactions
      .create_guild_command(guild_id)
      .chat_input("pause", "Play or pause a track")?
      .description_localizations(&localizations! {
        "ru" => "Поставить трек на паузу"
      })?
      .await?;

    interactions
      .create_guild_command(guild_id)
      .chat_input("filters", "FILTERS")?
      .description_localizations(&localizations! {
        "ru" => "Фильтры"
      })?
      .command_options(&[
        argument!(
          StringBuilder,
          "filters",
          "Filter graph definition",
          required(true),
          description_localizations(&localizations! {
            "ru" => "Описание графа фильтров"
          })
        )
      ])?
      .await?;

    interactions
      .create_guild_command(guild_id)
      .chat_input("queue", "Show queue")?
      .description_localizations(&localizations! {
        "ru" => "Show queue)"
      })?
      .await?;

    interactions
      .create_guild_command(guild_id)
      .chat_input("debug", "Show debug information")?
      .description_localizations(&localizations! {
        "ru" => "Show debug information)"
      })?
      .await?;

    interactions
      .create_guild_command(guild_id)
      .chat_input("seek", "Seek")?
      .description_localizations(&localizations! {
        "ru" => "Hide and seek"
      })?
      .command_options(&[
        argument!(
          StringBuilder,
          "position",
          "Absolute or relative (++/-) position",
          required(true)
        )
      ])?
      .await?;

    interactions
      .create_guild_command(guild_id)
      .chat_input("jump", "Jump to track")?
      .description_localizations(&localizations! {
        "ru" => "JMP ptr16:32"
      })?
      .command_options(&[
        argument!(
          StringBuilder,
          "position",
          "Absolute or relative (++/-) position in queue",
          required(true)
        )
      ])?
      .await?;

    let intents = Intents::GUILDS | Intents::GUILD_VOICE_STATES;
    let shard = Shard::new(ShardId::ONE, token, intents);

    let sender = shard.sender();

    (
      shard,
      Arc::new(StateRef {
        sender,
        http,
        cache,
        application_id,
        players: Default::default(),
        standby: Standby::new(),
      })
    )
  };

  let handlers: &mut HashMap<&'static str, Box<dyn CommandHandler>> = Box::leak(Box::new(HashMap::from([ // TODO(Assasans): Memory leak
    ("play", Box::new(PlayCommand {}) as Box<dyn CommandHandler>),
    ("pause", Box::new(PauseCommand {}) as Box<dyn CommandHandler>),
    ("filters", Box::new(FiltersCommand {}) as Box<dyn CommandHandler>),
    ("queue", Box::new(QueueCommand {}) as Box<dyn CommandHandler>),
    ("debug", Box::new(DebugCommand {}) as Box<dyn CommandHandler>),
    ("seek", Box::new(SeekCommand {}) as Box<dyn CommandHandler>),
    ("jump", Box::new(JumpCommand {}) as Box<dyn CommandHandler>),
  ])));

  while let Ok(event) = shard.next_event().await {
    state.standby.process(&event);
    state.cache.update(&event);

    if let Event::InteractionCreate(interaction) = event {
      let command = try_unpack!(interaction.data.as_ref().context("no interaction data")?, InteractionData::ApplicationCommand)?;
      let command_name = command.name.clone();

      match handlers.get(command.name.as_str()) {
        Some(handler) => {
          let cloned = state.clone();
          let state = state.clone();
          tokio::spawn(async move {
            let result = handler.run(cloned, interaction.as_ref()).await;
            if let Err(error) = result {
              tracing::debug!("handler error: {:?}", error);

              let mut fmt = String::new();

              let backtrace = error.backtrace().to_string();
              let regex = Regex::new(r"(\d+): (.+)\n\s*at (.+)(?::(\d+):(\d+))+?").unwrap();

              let mut skipped = 0;
              for capture in regex.captures_iter(&backtrace) {
                let index = capture.get(1).unwrap().as_str().parse::<i32>().unwrap();
                let frame = capture.get(2).unwrap().as_str();
                let file = capture.get(3).unwrap().as_str();
                let line = capture.get(4).map(|it| it.as_str()).unwrap_or("?");
                let column = capture.get(5).map(|it| it.as_str()).unwrap_or("?");

                if index >= 13 {
                  skipped += 1;
                  continue;
                }

                let color = if !file.contains("/rustc/") && !file.contains("/.cargo/") {
                  "33"
                } else {
                  "30"
                };
                fmt.write_fmt(format_args!("\u{001b}[2;34m{index:>2}: \u{001b}[2;{color}m{frame}\u{001b}[0m")).unwrap();
                fmt.push_str("\n");
                if !file.contains("/rustc/") && !file.contains("/.cargo/") {
                  fmt.write_fmt(format_args!("    at \u{001b}[1;2m{file}\u{001b}[0m:{line}:{column}")).unwrap();
                  fmt.push_str("\n");
                }
              }

              if skipped > 0 {
                fmt.write_fmt(format_args!("    \u{001b}[2;32m{skipped} more frames...\u{001b}[0m")).unwrap();
              }

              println!("{}", fmt);
              let r = format!("Handler `{}` error: {}\n```ansi\n{}```", command_name, error, fmt);
              println!("{} / {}", r.len(), r.chars().count());
              state
                .http
                .interaction(state.application_id)
                .update_response(&interaction.token)
                .content(Some(&r)).unwrap()
                .await.unwrap();
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
    }
  }

  Ok(())
}
