use std::borrow::ToOwned;
use std::collections::HashMap;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use debug_ignore::DebugIgnore;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::debug;

use voice::provider::SampleProvider;
use crate::providers::metadata;
use super::{MediaMetadata, MediaProvider, FFmpegMediaProvider};

#[derive(Debug)]
pub struct SberzvukMediaProvider {
  id: i64,
  stream: Option<Stream>
}

impl SberzvukMediaProvider {
  pub fn new(id: i64) -> Self {
    Self {
      id,
      stream: None
    }
  }
}

#[async_trait]
impl MediaProvider for SberzvukMediaProvider {
  async fn init(&mut self) -> Result<()> {
    let client = Client::new();
    let profile = client.get("https://zvuk.com/api/tiny/profile")
      .send().await?
      .json::<ProfileWrapper>().await?;
    debug!("token: {}", profile.result.token);

    let body = serde_json::to_string(&GraphQlRequest {
      operation_name: "getStream".to_owned(),
      variables: HashMap::from([
        ("ids".to_string(), vec![self.id].into())
      ]),
      query: GET_STREAM_QUERY
    })?;
    debug!("request body: {}", body);

    let response = client.post("https://zvuk.com/api/v1/graphql")
      .header("Content-Type", "application/json")
      .header("X-Auth-Token", profile.result.token)
      .body(body)
      .send().await?;
    let body = response.text().await?;
    debug!("response: {}", body);

    let mut body = serde_json::from_str::<ResponseWrapper<GetStreamResponse>>(&body)?;

    let content = body.data.media_contents.swap_remove(0);
    self.stream = Some(content.stream);

    Ok(())
  }

  async fn get_sample_provider(&self) -> Result<Box<dyn SampleProvider>> {
    let stream = match self.stream {
      Some(ref stream) => stream,
      None => return Err(anyhow!("media provider is not initialized"))
    };

    let url = stream.high.as_ref().unwrap_or(&stream.mid);

    let inner = FFmpegMediaProvider::new(url.clone());
    inner.get_sample_provider().await
  }

  async fn get_metadata(&self) -> Result<Vec<MediaMetadata>> {
    Ok(metadata! {
      Id => { Some(self.id.to_string()) }
    })
  }
}

static GET_STREAM_QUERY: &str = r#"query getStream($ids: [ID!]!) {
  mediaContents(ids: $ids) {
    ... on Track {
      stream {
        expire
        expireDelta
        flacdrm
        high
        mid
      }
    }
    ... on Episode {
      stream {
        expire
        expireDelta
        high
        mid
      }
    }
    ... on Chapter {
      stream {
        expire
        expireDelta
        high
        mid
      }
    }
  }
}"#;

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
pub struct ProfileWrapper {
  pub result: Profile
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
pub struct Profile {
  pub id: i64,
  pub is_anonymous: bool,
  pub token: String
}

#[derive(Default, Debug, Clone, PartialEq, Serialize)]
pub struct GraphQlRequest {
  #[serde(rename = "operationName")]
  pub operation_name: String,
  pub variables: HashMap<String, Value>,
  pub query: &'static str
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
pub struct ResponseWrapper<T> {
  pub data: T
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
pub struct GetStreamResponse {
  #[serde(rename = "mediaContents")]
  pub media_contents: Vec<MediaContent>
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
pub struct MediaContent {
  pub stream: Stream
}

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
pub struct Stream {
  pub expire: String,
  #[serde(rename = "expireDelta")]
  pub expire_delta: i64,
  pub flacdrm: Option<String>,
  pub high: Option<String>,
  pub mid: String,
}
