mod metadata;
pub mod file;
pub mod http;
pub mod yt_dlp;

pub use metadata::*;

use std::fmt::Debug;
use anyhow::Result;
use async_trait::async_trait;
use songbird::input::Input;

#[async_trait]
pub trait MediaProvider: Sync + Send + Debug {
  async fn to_input(&self) -> Result<Input>;
  async fn get_metadata(&self) -> Result<Vec<MediaMetadata>>;
}
