mod metadata;
mod file;
mod http;
pub mod async_adapter;

pub use metadata::*;
pub use file::*;
pub use http::*;

use std::fmt::Debug;
use anyhow::Result;
use async_trait::async_trait;

use voice::provider::SampleProvider;

#[async_trait]
pub trait MediaProvider: Sync + Send + Debug {
  async fn get_sample_provider(&self) -> Result<Box<dyn SampleProvider>>;
  async fn get_metadata(&self) -> Result<Vec<MediaMetadata>>;
}
