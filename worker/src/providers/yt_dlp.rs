use std::fmt::Debug;

use anyhow::Result;
use async_trait::async_trait;
use songbird::input::Input;
use tokio::process::Command;

use super::{MediaProvider, http::HttpMediaProvider};

pub struct YoutubeDlMediaProvider {
  url: String
}

impl YoutubeDlMediaProvider {
  pub fn new(url: String) -> Self {
    Self { url }
  }
}

#[async_trait]
impl MediaProvider for YoutubeDlMediaProvider {
  async fn to_input(&self) -> Result<Input> {
    let output = Command::new("yt-dlp")
      .arg("--get-url")
      .arg(self.url.clone())
      .output()
      .await?;

    let stdout = String::from_utf8(output.stdout).unwrap();
    let urls: Vec<&str> = stdout.trim().split('\n').collect();
    let url = urls[1];

    println!("Audio URL: {:?}", url);

    let http = HttpMediaProvider::new(url.to_owned());
    http.to_input().await
  }
}

impl Debug for YoutubeDlMediaProvider {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "YoutubeDlMediaProvider {{ url: {} }}", self.url)
  }
}
