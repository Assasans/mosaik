use std::borrow::ToOwned;
use std::cmp::Ordering;
use std::process::Stdio;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use debug_ignore::DebugIgnore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::process::Command;
use tracing::debug;
use voice::provider::SampleProvider;

use super::{metadata, FFmpegMediaProvider, MediaMetadata, MediaProvider};

#[derive(Debug)]
pub struct YtDlpMediaProvider {
  query: String,
  data: Option<DebugIgnore<Value>>
}

impl YtDlpMediaProvider {
  pub fn new(query: String) -> Self {
    Self { query, data: None }
  }
}

#[async_trait]
impl MediaProvider for YtDlpMediaProvider {
  async fn init(&mut self) -> Result<()> {
    let output = Command::new("yt-dlp")
      .args(&["--no-download", "--print-json", &self.query])
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .stdin(Stdio::piped())
      .spawn()?
      .wait_with_output()
      .await?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    let data = self.data.insert(serde_json::from_str::<Value>(&stdout)?.into());
    debug!("yt-dlp media provider initialized: {:?}", data);

    Ok(())
  }

  async fn get_sample_provider(&self) -> Result<Box<dyn SampleProvider>> {
    let data = match self.data {
      Some(ref data) => data,
      None => return Err(anyhow!("media provider is not initialized"))
    };

    let none = "none".to_owned();
    let mut formats = serde_json::from_value::<Vec<Format>>(data["formats"].clone())?;
    formats.sort_by(|a, b| {
      Ordering::Equal
        // Prefer format with audio
        .then_with(|| {
          let a = a.acodec.as_ref().unwrap_or(&"".to_owned()) != &none;
          let b = b.acodec.as_ref().unwrap_or(&"".to_owned()) != &none;
          b.cmp(&a)
        })
        // Prefer Opus
        .then_with(|| {
          let a = match a.acodec {
            Some(ref codec) => codec.as_str() == "opus",
            None => return Ordering::Less
          };
          let b = match b.acodec {
            Some(ref codec) => codec.as_str() == "opus",
            None => return Ordering::Less
          };

          b.cmp(&a)
        })
        // Prefer highest audio bitrate
        .then_with(|| {
          let a = a.abr.unwrap_or(0.0);
          let b = b.abr.unwrap_or(0.0);
          b.total_cmp(&a)
        })
        // Prefer format without video
        .then_with(|| {
          let a = a.vcodec.as_ref().unwrap_or(&none) == &none;
          let b = b.vcodec.as_ref().unwrap_or(&none) == &none;
          b.cmp(&a)
        })
    });

    let format = formats.first().unwrap();
    debug!("using format {:?} for {}", format, self.query);

    let inner = FFmpegMediaProvider::new(format.url.to_owned());
    inner.get_sample_provider().await
  }

  async fn get_metadata(&self) -> Result<Vec<MediaMetadata>> {
    let data = match self.data {
      Some(ref data) => data,
      None => return Err(anyhow!("media provider is not initialized"))
    };

    Ok(metadata! {
      Id => { data["id"].as_str() },
      Title => { data["title"].as_str() },
      Url => { data["original_url"].as_str() },
    })
  }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Format {
  pub filesize: Option<i64>,
  pub format: String,
  pub format_id: String,
  pub format_note: Option<String>,
  pub audio_channels: Option<i64>,
  pub url: String,
  pub language: Option<String>,
  pub ext: Option<String>,
  pub vcodec: Option<String>,
  pub acodec: Option<String>,
  pub container: Option<String>,
  pub protocol: Option<String>,
  pub audio_ext: Option<String>,
  pub video_ext: Option<String>,
  pub vbr: Option<f64>,
  pub abr: Option<f64>
}
