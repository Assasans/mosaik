use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use songbird::input::{Input, HttpRequest};

use super::MediaProvider;

pub struct HttpMediaProvider {
  url: String
}

impl HttpMediaProvider {
  pub fn new(url: String) -> Self {
    Self { url }
  }
}

#[async_trait]
impl MediaProvider for HttpMediaProvider {
  async fn to_input(&self) -> Result<Input> {
    let client = Client::new(); // TODO(Assasans): Shared
    Ok(Input::Lazy(Box::new(HttpRequest::new(client, self.url.clone()))))
  }
}
