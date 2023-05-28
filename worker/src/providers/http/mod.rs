use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use symphonia::core::probe::Hint;
use tokio::sync::oneshot;
use tracing::info;

use voice::provider::SampleProvider;
use crate::{
  voice::SymphoniaSampleProvider,
  providers::{MediaMetadata, MediaProvider}
};
use self::request::HttpRequest;

pub mod request;

#[derive(Debug)]
pub struct SeekableHttpMediaProvider {
  request: String
}

impl SeekableHttpMediaProvider {
  pub fn new(request: String) -> Self {
    Self {
      request
    }
  }
}

#[async_trait]
impl MediaProvider for SeekableHttpMediaProvider {
  async fn get_sample_provider(&self) -> Result<Box<dyn SampleProvider>> {
    let client = Client::new();
    let mut request = HttpRequest::new(client, self.request.clone());
    let stream = request.create_async().await.unwrap();

    let (tx, rx) = oneshot::channel();
    tokio::task::spawn_blocking(move || {
      info!("waiting for sample provider...");
      tx.send(SymphoniaSampleProvider::new_from_source(
        stream.input,
        stream.hint.unwrap_or_default()
      ).unwrap()).unwrap();
    });

    Ok(Box::new(rx.await?))
  }

  async fn get_metadata(&self) -> Result<Vec<MediaMetadata>> {
    // TODO: Implement the logic to extract metadata from the file
    Ok(vec![])
  }
}
