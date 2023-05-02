use std::{fs::File, path::{PathBuf, Path}};
use anyhow::Result;
use async_trait::async_trait;
use symphonia::core::probe::Hint;
use voice::provider::SampleProvider;

use crate::voice::SymphoniaSampleProvider;

use super::{MediaProvider, MediaMetadata};

#[derive(Debug)]
pub struct FileMediaProvider {
  path: PathBuf
}

impl FileMediaProvider {
  pub fn new<P: AsRef<Path>>(path: P) -> Self {
    Self {
      path: path.as_ref().to_owned()
    }
  }
}

#[async_trait]
impl MediaProvider for FileMediaProvider {
  async fn get_sample_provider(&self) -> Result<Box<dyn SampleProvider>> {
    let file = File::open(self.path.clone())?;
    let mut hint = Hint::new();
    if let Some(extension) = self.path.extension().and_then(|it| it.to_str()) {
      hint.with_extension(extension);
    }

    Ok(Box::new(SymphoniaSampleProvider::new_from_source(Box::new(file), hint)?))
  }

  async fn get_metadata(&self) -> Result<Vec<MediaMetadata>> {
    // TODO: Implement the logic to extract metadata from the file
    Ok(vec![])
  }
}
