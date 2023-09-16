mod metadata;
mod ffmpeg;
mod yt_dlp;

pub use metadata::*;
pub use ffmpeg::*;
pub use yt_dlp::*;

use std::fmt::Debug;
use anyhow::Result;
use async_trait::async_trait;

use voice::provider::SampleProvider;

#[async_trait]
pub trait MediaProvider: Sync + Send + Debug {
  async fn get_sample_provider(&self) -> Result<Box<dyn SampleProvider>>;
  async fn get_metadata(&self) -> Result<Vec<MediaMetadata>>;
}
