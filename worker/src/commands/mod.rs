mod play;
mod pause;

pub use play::*;
pub use pause::*;

use anyhow::Result;
use async_trait::async_trait;
use twilight_model::gateway::payload::incoming::InteractionCreate;

use crate::State;

#[async_trait]
pub trait CommandHandler: Sync {
  async fn run(&self, state: State, interaction: Box<InteractionCreate>) -> Result<()>;
}
