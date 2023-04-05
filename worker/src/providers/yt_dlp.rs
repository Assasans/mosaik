use std::{fmt::Debug, time::Duration};

use anyhow::Result;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use songbird::input::Input;
use tokio::process::Command;

use super::{MediaProvider, http::HttpMediaProvider, MediaMetadata};

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

  async fn get_metadata(&self) -> Result<Vec<MediaMetadata>> {
    // TODO(Assasans): Do not invoke 2 times
    let output = Command::new("yt-dlp")
      .arg("--no-download")
      .arg("--print-json")
      .arg(self.url.clone())
      .output()
      .await?;

    let stdout = String::from_utf8(output.stdout).unwrap();
    let video: VideoInfo = serde_json::from_str(&stdout)?;

    Ok(vec![
      MediaMetadata::Id(video.id),
      MediaMetadata::Title(video.title),
      MediaMetadata::Thumbnail(video.thumbnail),
      MediaMetadata::Description(video.description),
      MediaMetadata::Duration(Duration::from_secs(video.duration)),
      MediaMetadata::ViewCount(video.view_count)
    ])
  }
}

impl Debug for YoutubeDlMediaProvider {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "YoutubeDlMediaProvider {{ url: {} }}", self.url)
  }
}

#[derive(Debug, Serialize, Deserialize)]
struct VideoInfo {
  pub id: String,
  pub title: String,
  pub thumbnail: String,
  pub description: String,
  pub duration: u64,
  pub view_count: u64,
  pub requested_formats: Vec<VideoFormat>
}

#[derive(Debug, Serialize, Deserialize)]
struct VideoFormat {
  pub format_id: String,
  pub url: String
}
