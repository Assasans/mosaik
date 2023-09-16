use tracing::debug;
use decoder::Decoder;
use voice::provider::SampleProvider;

pub struct FFmpegSampleProvider {
  pub decoder: Decoder,
  flushing: bool
}

impl FFmpegSampleProvider {
  pub fn new() -> Self {
    Self {
      decoder: Decoder::new(),
      flushing: false
    }
  }

  pub fn open(&mut self, path: &str) {
    self.decoder.open_input(path);
  }

  pub fn init_filters(&mut self, description: &str) {
    self.decoder.init_filters(description);
  }
}

impl SampleProvider for FFmpegSampleProvider {
  fn get_samples(&mut self) -> Option<Vec<f32>> {
    match self.decoder.read_frame(self.flushing) {
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
}
