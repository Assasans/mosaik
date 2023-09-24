mod play;
mod pause;
mod filters;
mod queue;
mod debug;
mod seek;
mod jump;

pub use play::*;
pub use pause::*;
pub use filters::*;
pub use queue::*;
pub use debug::*;
pub use seek::*;
pub use jump::*;

use anyhow::Result;
use async_trait::async_trait;
use twilight_model::gateway::payload::incoming::InteractionCreate;

use crate::State;

#[async_trait]
pub trait CommandHandler: Sync {
  async fn run(&self, state: State, interaction: &InteractionCreate) -> Result<()>;
}
