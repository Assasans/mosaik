use std::process::Stdio;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use tokio::process::Command;
use tracing::debug;

use voice::provider::SampleProvider;
use crate::providers::FFmpegMediaProvider;
use super::{MediaMetadata, MediaProvider};

#[derive(Debug)]
pub struct YtDlpMediaProvider {
  query: String
}

impl YtDlpMediaProvider {
  pub fn new(query: String) -> Self {
    Self { query }
  }
}

macro_rules! metadata {
  ($($kind:ident => $block:block),*$(,)?) => {{
    let mut metadata = Vec::new();
    $(
      if let Some(value) = $block {
        metadata.push(MediaMetadata::$kind(value.to_owned()));
      }
    )*
    metadata
  }};
}

#[async_trait]
impl MediaProvider for YtDlpMediaProvider {
  async fn get_sample_provider(&self) -> Result<Box<dyn SampleProvider>> {
    let output = Command::new("yt-dlp")
      .args(&["--no-download", "--get-url", &self.query])
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .stdin(Stdio::piped())
      .spawn()?
      .wait_with_output().await?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let urls = stdout.split("\n").collect::<Vec<_>>();
    let url = if urls.len() == 2 {
      urls[0] // YouTube live streams or other services
    } else {
      urls[1] // YouTube videos (split audio and video streams)
    };
    debug!("using url {} for {}", url, self.query);

    let inner = FFmpegMediaProvider::new(url.to_owned());
    inner.get_sample_provider().await
  }

  async fn get_metadata(&self) -> Result<Vec<MediaMetadata>> {
    let output = Command::new("yt-dlp")
      .args(&["--no-download", "--print-json", &self.query])
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .stdin(Stdio::piped())
      .spawn()?
      .wait_with_output().await?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let data: Value = serde_json::from_str(&stdout)?;
    debug!("data: {:#?}", data);

    Ok(metadata! {
      Id => { data["id"].as_str() },
      Title => { data["title"].as_str() },
      Url => { data["original_url"].as_str() },
    })
  }
}
