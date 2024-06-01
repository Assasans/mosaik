use std::borrow::ToOwned;
use std::cmp::Ordering;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use debug_ignore::DebugIgnore;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::process::Command;
use tracing::debug;

use voice::provider::SampleProvider;

use crate::providers::YtDlpMediaProvider;

use super::{MediaProvider, MediaProviderFactory};

#[derive(Debug)]
pub struct YtDlpPlaylistMediaProviderFactory {
  query: String,
  data: Option<DebugIgnore<Vec<Value>>>
}

impl YtDlpPlaylistMediaProviderFactory {
  pub fn new(query: String) -> Self {
    Self { query, data: None }
  }
}

#[async_trait]
impl MediaProviderFactory for YtDlpPlaylistMediaProviderFactory {
  async fn init(&mut self) -> Result<()> {
    let output = Command::new("yt-dlp")
      .args(&["--no-download", "--print-json", "--flat-playlist", &self.query])
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .stdin(Stdio::piped())
      .spawn()?
      .wait_with_output()
      .await?;
    if !output.status.success() {
      let stderr = String::from_utf8_lossy(&output.stderr);
      debug!("yt-dlp media provider error: {:?}", stderr);
      return Err(anyhow!("yt-dlp exit code {:?}: {}", output.status.code(), stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let deserializer = serde_json::Deserializer::from_str(&stdout);
    let data = deserializer.into_iter::<Value>().flatten().collect::<Vec<_>>();

    let data = self.data.insert(data.into());
    debug!("yt-dlp media provider initialized: {:?}", data);

    Ok(())
  }

  async fn get_media_providers(&self) -> Result<Vec<Box<dyn MediaProvider>>> {
    let data = match self.data {
      Some(ref data) => data,
      None => return Err(anyhow!("media provider factory is not initialized"))
    };

    let mut providers = Vec::<Box<dyn MediaProvider>>::new();
    for item in &data.0 {
      let item = serde_json::from_value::<Item>(item.to_owned()).unwrap();
      debug!("item {:?} in {}", item, self.query);

      let inner = YtDlpMediaProvider::new(item.url.to_owned());
      providers.push(Box::new(inner));
    }

    Ok(providers)
  }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
  pub id: String,
  pub title: String,
  pub duration: Option<u64>,
  pub url: String,
}
