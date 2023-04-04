mod join;
mod play;

pub use join::*;
pub use play::*;

use anyhow::Result;
use async_trait::async_trait;
use twilight_model::{gateway::payload::incoming::InteractionCreate, application::interaction::application_command::CommandData};

use crate::State;

#[async_trait]
pub trait CommandHandler: Sync {
  async fn run(&self, state: State, interaction: Box<InteractionCreate>) -> Result<()>;
}
