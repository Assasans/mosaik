use std::borrow::ToOwned;
use std::collections::HashMap;
use std::time::Duration;

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
  track: Option<DebugIgnore<GetTrack>>,
  stream: Option<Stream>
}

impl SberzvukMediaProvider {
  pub fn new(id: i64) -> Self {
    Self {
      id,
      track: None,
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
      .header("X-Auth-Token", &profile.result.token)
      .body(body)
      .send().await?;
    let body = response.text().await?;
    debug!("response: {}", body);

    let mut body = serde_json::from_str::<ResponseWrapper<GetStreamResponse>>(&body)?;

    self.track = Some({
      let body = serde_json::to_string(&GraphQlRequest {
        operation_name: "getFullTrack".to_owned(),
        variables: HashMap::from([
          ("ids".to_owned(), vec![self.id].into()),
          ("withArtists".to_owned(), true.into()),
          ("withReleases".to_owned(), true.into())
        ]),
        query: GET_TRACK_QUERY
      })?;
      debug!("request body: {}", body);

      let response = client.post("https://zvuk.com/api/v1/graphql")
        .header("Content-Type", "application/json")
        .header("X-Auth-Token", &profile.result.token)
        .body(body)
        .send().await?;
      let body = response.text().await?;
      debug!("response: {}", body);

      let mut body = serde_json::from_str::<ResponseWrapper<GetTrackResponse>>(&body)?;
      body.data.get_tracks.swap_remove(0)
    }.into());

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
      Id => { Some(self.id.to_string()) },
      Title => { self.track.as_ref().map(|track| track.title.clone()) },
      Duration => { self.track.as_ref().map(|track| Duration::from_secs(track.duration)) }
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

static GET_TRACK_QUERY: &str = r#"query getFullTrack($ids: [ID!]!, $withReleases: Boolean = false, $withArtists: Boolean = false) {
  getTracks(ids: $ids) {
    id
    title
    searchTitle
    position
    duration
    availability
    artistTemplate
    condition
    explicit
    lyrics
    zchan
    collectionItemData {
      itemStatus
    }
    artists @include(if: $withArtists) {
      id
      title
      searchTitle
      description
      hasPage
      image {
        src
        palette
        paletteBottom
      }
      secondImage {
        src
        palette
        paletteBottom
      }
      animation {
        artistId
        effect
        image
        background {
          type
          image
          color
          gradient
        }
      }
    }
    release @include(if: $withReleases) {
      id
      title
      searchTitle
      type
      date
      image {
        src
        palette
        paletteBottom
      }
      genres {
        id
        name
        shortName
      }
      label {
        id
        title
      }
      availability
      artistTemplate
    }
    hasFlac
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

#[derive(Default, Debug, Clone, PartialEq, Deserialize)]
pub struct GetTrackResponse {
  #[serde(rename = "getTracks")]
  pub get_tracks: Vec<GetTrack>
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GetTrack {
  pub id: String,
  pub title: String,
  #[serde(rename = "searchTitle")]
  pub search_title: String,
  pub position: i64,
  pub duration: u64,
  pub availability: i64,
  #[serde(rename = "artistTemplate")]
  pub artist_template: String,
  pub condition: String,
  pub explicit: bool,
  pub lyrics: bool,
  pub zchan: String,
  #[serde(rename = "collectionItemData")]
  pub collection_item_data: CollectionItemData,
  pub artists: Vec<Artist>,
  pub release: Release,
  #[serde(rename = "hasFlac")]
  pub has_flac: bool
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionItemData {
  #[serde(rename = "itemStatus")]
  pub item_status: Value
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artist {
  pub id: String,
  pub title: String,
  #[serde(rename = "searchTitle")]
  pub search_title: String,
  pub description: String,
  #[serde(rename = "hasPage")]
  pub has_page: bool,
  pub image: Image,
  #[serde(rename = "secondImage")]
  pub second_image: SecondImage,
  pub animation: Value
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Image {
  pub src: String,
  pub palette: String,
  #[serde(rename = "paletteBottom")]
  pub palette_bottom: Value
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecondImage {
  pub src: String,
  pub palette: Value,
  #[serde(rename = "paletteBottom")]
  pub palette_bottom: Value
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Release {
  pub id: String,
  pub title: String,
  #[serde(rename = "searchTitle")]
  pub search_title: String,
  #[serde(rename = "type")]
  pub type_field: String,
  pub date: String,
  pub image: Image2,
  pub genres: Vec<Genre>,
  pub label: Label,
  pub availability: i64,
  #[serde(rename = "artistTemplate")]
  pub artist_template: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Image2 {
  pub src: String,
  pub palette: String,
  #[serde(rename = "paletteBottom")]
  pub palette_bottom: String
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Genre {
  pub id: String,
  pub name: String,
  #[serde(rename = "shortName")]
  pub short_name: Value
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Label {
  pub id: String,
  pub title: String
}
