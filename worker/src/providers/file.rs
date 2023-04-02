use anyhow::Result;
use async_trait::async_trait;
use songbird::input::{File, Input};

use super::MediaProvider;

pub struct FileMediaProvider {
  path: String
}

impl FileMediaProvider {
  pub fn new(path: String) -> Self {
    Self { path }
  }
}

#[async_trait]
impl MediaProvider for FileMediaProvider {
  async fn to_input(&self) -> Result<Input> {
    Ok(Input::Lazy(Box::new(File::new(self.path.clone()))))
  }
}
