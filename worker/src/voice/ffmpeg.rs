use std::any::Any;
use std::sync::{Arc, Mutex};
use anyhow::anyhow;
use tracing::debug;
use decoder::{Decoder, RawError};
use voice::provider::{SampleProvider, SampleProviderHandle};

pub struct FFmpegSampleProvider {
  pub decoder: Arc<Mutex<Decoder>>,
  flushing: bool
}

impl FFmpegSampleProvider {
  pub fn new() -> Self {
    Self {
      decoder: Arc::new(Mutex::new(Decoder::new())),
      flushing: false
    }
  }

  pub fn open(&mut self, path: &str) -> anyhow::Result<()> {
    let mut decoder = self.decoder.lock().unwrap();
    decoder.open_input(path).map_err(|code| anyhow!("ffmpeg error {}", code))
  }

  pub fn init_filters(&mut self, description: &str) -> Result<(), RawError> {
    let mut decoder = self.decoder.lock().unwrap();
    decoder.init_filters(description)
  }
}

impl SampleProvider for FFmpegSampleProvider {
  fn get_samples(&mut self) -> Option<Vec<f32>> {
    let mut decoder = self.decoder.lock().unwrap();
    match decoder.read_frame(self.flushing) {
      Some(read) => {
        Some(read)
      },
      None => {
        if !self.flushing {
          debug!("flushing decoder...");
          self.flushing = true;
          return Some(Vec::new()); // Request retry
        }

        None
      }
    }
  }

  fn as_any(&mut self) -> &mut (dyn Any + Sync + Send) {
    self
  }

  fn get_handle(&self) -> Box<dyn SampleProviderHandle> {
    Box::new(FFmpegSampleProviderHandle {
      decoder: self.decoder.clone()
    })
  }
}

pub struct FFmpegSampleProviderHandle {
  pub decoder: Arc<Mutex<Decoder>>,
}

impl SampleProviderHandle for FFmpegSampleProviderHandle {
  fn as_any(&self) -> &(dyn Any + Sync + Send) {
    self
  }
}

impl FFmpegSampleProviderHandle {
  pub fn set_enable_filter_graph(&self, enable: bool) -> Result<(), RawError> {
    let mut decoder = self.decoder.lock().unwrap();
    decoder.set_enable_filter_graph(enable)
  }

  pub fn init_filters(&self, description: &str) -> Result<(), RawError> {
    let mut decoder = self.decoder.lock().unwrap();
    decoder.init_filters(description)
  }
}
