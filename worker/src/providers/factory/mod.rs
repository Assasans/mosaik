mod yt_dlp_playlist;

pub use yt_dlp_playlist::*;

use std::fmt::Debug;
use async_trait::async_trait;
use crate::providers::MediaProvider;

#[async_trait]
pub trait MediaProviderFactory: Sync + Send + Debug {
  async fn init(&mut self) -> anyhow::Result<()> {
    Ok(())
  }

  async fn get_media_providers(&self) -> anyhow::Result<Vec<Box<dyn MediaProvider>>>;
}
