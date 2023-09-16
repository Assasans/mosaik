use std::{path::{Path, PathBuf}};

use anyhow::Result;
use async_trait::async_trait;

use voice::provider::SampleProvider;
use super::{MediaMetadata, MediaProvider};
use crate::voice::ffmpeg::FFmpegSampleProvider;

#[derive(Debug)]
pub struct FFmpegMediaProvider {
  path: String
}

impl FFmpegMediaProvider {
  pub fn new(path: String) -> Self {
    Self {
      path
    }
  }
}

#[async_trait]
impl MediaProvider for FFmpegMediaProvider {
  async fn get_sample_provider(&self) -> Result<Box<dyn SampleProvider>> {
    let mut provider = FFmpegSampleProvider::new();
    provider.open(&self.path).unwrap();
    provider.init_filters("anull").unwrap();
    Ok(Box::new(provider))
  }

  async fn get_metadata(&self) -> Result<Vec<MediaMetadata>> {
    // TODO: Implement the logic to extract metadata from the file
    Ok(vec![])
  }
}
