use std::any::Any;
use std::sync::{Arc, Mutex};
use tracing::debug;
use decoder::Decoder;
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

  pub fn open(&mut self, path: &str) {
    let mut decoder = self.decoder.lock().unwrap();
    decoder.open_input(path);
  }

  pub fn init_filters(&mut self, description: &str) {
    let mut decoder = self.decoder.lock().unwrap();
    decoder.init_filters(description);
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

  fn as_any(&mut self) -> &mut dyn Any {
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
  fn as_any(&self) -> &dyn Any {
    self
  }
}

impl FFmpegSampleProviderHandle {
  pub fn init_filters(&self, description: &str) {
    let mut decoder = self.decoder.lock().unwrap();
    decoder.init_filters(description);
  }
}