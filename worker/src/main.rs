pub mod commands;
pub mod player;
pub mod providers;
pub mod util;
pub mod voice;
mod provider_predictor;

use std::env;
use std::error::Error;
use std::fmt::Write;
use std::future::Future;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use regex::Regex;
use serenity::all::GuildId;
use serenity::prelude::*;
use tracing::{error, info};
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::voice::MosaikVoiceManager;

include_and_export!(state);

fn spawn(fut: impl Future<Output = Result<(), Box<dyn Error + Send + Sync + 'static>>> + Send + 'static) {
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

type AnyError = anyhow::Error; // Box<dyn Error + Send + Sync>;
type PoiseContext<'a> = poise::Context<'a, State, AnyError>;

fn pretty_print_error(error: anyhow::Error) -> String {
  let mut fmt = String::new();
  fmt
    .write_fmt(format_args!("\u{001b}[2;31m{}\u{001b}[0m\n", error))
    .unwrap();

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
    fmt
      .write_fmt(format_args!(
        "\u{001b}[2;34m{index:>2}: \u{001b}[2;{color}m{frame}\u{001b}[0m"
      ))
      .unwrap();
    fmt.push_str("\n");
    if !file.contains("/rustc/") && !file.contains("/.cargo/") {
      fmt
        .write_fmt(format_args!("    at \u{001b}[1;2m{file}\u{001b}[0m:{line}:{column}"))
        .unwrap();
      fmt.push_str("\n");
    }
  }

  if skipped > 0 {
    fmt
      .write_fmt(format_args!("    \u{001b}[2;32m{skipped} more frames...\u{001b}[0m"))
      .unwrap();
  }

  return fmt;
}

async fn on_error(error: poise::FrameworkError<'_, State, AnyError>) {
  // This is our custom error handler
  // They are many errors that can occur, so we only handle the ones we want to customize
  // and forward the rest to the default handler
  match error {
    poise::FrameworkError::Setup { error, .. } => panic!("Failed to start bot: {:?}", error),
    poise::FrameworkError::Command { error, ctx, .. } => {
      error!("Error in command `{}`: {:?}", ctx.command().name, error);
      ctx
        .reply(format!(
          "Error in command `{}`:```ansi\n{}\n```",
          ctx.command().name,
          pretty_print_error(error)
        ))
        .await
        .unwrap();
    }
    error => {
      if let Err(error) = poise::builtins::on_error(error).await {
        error!("Error while handling error: {}", error)
      }
    }
  }
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
  info!("hello");

  let options = poise::FrameworkOptions {
    commands: vec![
      commands::help(),
      commands::play(),
      commands::filters(),
      commands::pause(),
      commands::seek(),
      commands::queue(),
      commands::debug(),
      commands::jump(),
    ],
    prefix_options: poise::PrefixFrameworkOptions {
      prefix: Some("~".into()),
      mention_as_prefix: true,
      edit_tracker: Some(Arc::new(poise::EditTracker::for_timespan(Duration::from_secs(600)))),
      ..Default::default()
    },
    // The global error handler for all error cases that may occur
    on_error: |error| Box::pin(on_error(error)),
    // This code is run before every command
    pre_command: |ctx| {
      Box::pin(async move {
        info!("Executing command {}...", ctx.command().qualified_name);
      })
    },
    // This code is run after a command if it was successful (returned Ok)
    post_command: |ctx| {
      Box::pin(async move {
        info!("Executed command {}!", ctx.command().qualified_name);
      })
    },
    // Every command invocation must pass this check to continue execution
    command_check: Some(|ctx| {
      Box::pin(async move {
        if ctx.author().id == 123456789 {
          return Ok(false);
        }
        Ok(true)
      })
    }),
    // Enforce command checks even for owners (enforced by default)
    // Set to true to bypass checks, which is useful for testing
    skip_checks_for_owners: false,
    event_handler: |_ctx, event, _framework, _data| {
      Box::pin(async move {
        info!("Got an event in event handler: {:?}", event.snake_case_name());
        Ok(())
      })
    },
    ..Default::default()
  };

  let framework = poise::Framework::builder()
    .setup(move |ctx, _ready, framework| {
      Box::pin(async move {
        info!("Logged in as {}", _ready.user.name);
        poise::builtins::register_in_guild(ctx, &framework.options().commands, GuildId::from(1171104054131314708))
          .await?;

        Ok(Arc::new(StateRef {
          players: Default::default()
        }))
      })
    })
    .options(options)
    .build();

  let voice_manager = Arc::new(MosaikVoiceManager::new());
  VOICE_MANAGER.set(voice_manager.clone()).unwrap();

  let token = env::var("DISCORD_TOKEN").expect("token");
  let intents = GatewayIntents::GUILDS
    | GatewayIntents::GUILD_VOICE_STATES
    | GatewayIntents::GUILD_MESSAGES
    | GatewayIntents::MESSAGE_CONTENT;
  let mut client = Client::builder(token, intents)
    .voice_manager_arc(voice_manager)
    .framework(framework)
    .await
    .expect("Error creating client");

  if let Err(why) = client.start().await {
    println!("An error occurred while running the client: {:?}", why);
  }

  Ok(())
}

pub static VOICE_MANAGER: OnceLock<Arc<MosaikVoiceManager>> = OnceLock::new();
