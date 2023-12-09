mod ffmpeg;
mod metadata;
mod sberzvuk;
mod vk;
mod yt_dlp;

use std::fmt::Debug;

use anyhow::Result;
use async_trait::async_trait;
pub use ffmpeg::*;
pub use metadata::*;
pub use sberzvuk::*;
pub use vk::*;
use voice::provider::SampleProvider;
pub use yt_dlp::*;

#[async_trait]
pub trait MediaProvider: Sync + Send + Debug {
  async fn init(&mut self) -> Result<()> {
    Ok(())
  }

  async fn get_sample_provider(&self) -> Result<Box<dyn SampleProvider>>;
  async fn get_metadata(&self) -> Result<Vec<MediaMetadata>>;
}
