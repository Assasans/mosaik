mod metadata;

pub use metadata::*;

use std::fmt::Debug;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait MediaProvider: Sync + Send + Debug {
  async fn to_input(&self) -> Result<()>;
  async fn get_metadata(&self) -> Result<Vec<MediaMetadata>>;
}
