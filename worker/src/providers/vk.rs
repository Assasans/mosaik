use std::borrow::ToOwned;
use std::env;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::debug;
use voice::provider::SampleProvider;

use super::{metadata, FFmpegMediaProvider, MediaMetadata, MediaProvider};

#[derive(Debug)]
pub struct VkMediaProvider {
  owner_id: i64,
  track_id: i64,
  track: Option<Track>
}

impl VkMediaProvider {
  pub fn new(owner_id: i64, track_id: i64) -> Self {
    Self {
      owner_id,
      track_id,
      track: None
    }
  }
}

#[async_trait]
impl MediaProvider for VkMediaProvider {
  async fn init(&mut self) -> Result<()> {
    let client = Client::new();
    let response = client
      .get("https://api.vk.com/method/audio.getById")
      .query(&[
        ("audios", format!("{}_{}", self.owner_id, self.track_id).as_str()),
        ("access_token", &env::var("VK_ACCESS_TOKEN").unwrap()),
        ("v", "5.221")
      ])
      .send()
      .await?;
    let body = response.text().await?;
    debug!("response: {}", body);

    let mut body = serde_json::from_str::<ResponseWrapper<Vec<Track>>>(&body)?;
    self.track = Some(body.response.swap_remove(0));

    Ok(())
  }

  async fn get_sample_provider(&self) -> Result<Box<dyn SampleProvider>> {
    let stream = match self.track {
      Some(ref stream) => stream,
      None => return Err(anyhow!("media provider is not initialized"))
    };

    let url = &stream.url;

    let inner = FFmpegMediaProvider::new(url.clone());
    inner.get_sample_provider().await
  }

  async fn get_metadata(&self) -> Result<Vec<MediaMetadata>> {
    Ok(metadata! {
      Id => { Some(self.track_id.to_string()) }
    })
  }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseWrapper<T> {
  pub response: T
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
  pub artist: String,
  pub id: i64,
  pub owner_id: i64,
  pub title: String,
  pub duration: i64,
  pub access_key: String,
  pub is_explicit: bool,
  pub is_focus_track: bool,
  pub is_licensed: bool,
  pub track_code: String,
  pub url: String,
  pub date: i64,
  pub genre_id: i64
}
