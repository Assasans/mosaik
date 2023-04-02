pub mod file;
pub mod http;
pub mod yt_dlp;

use anyhow::Result;
use async_trait::async_trait;
use songbird::input::Input;

#[async_trait]
pub trait MediaProvider {
  async fn to_input(&self) -> Result<Input>;
}
