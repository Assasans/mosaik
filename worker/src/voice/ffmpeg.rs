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
  fn get_samples(&mut self, samples: &mut [f32]) -> Option<usize> {
    match self.decoder.read_frame(self.flushing) {
      Some(read) => {
        samples[..read.len()].copy_from_slice(&read);
        Some(read.len())
      },
      None => {
        if !self.flushing {
          debug!("flushing decoder...");
          self.flushing = true;
          return Some(0); // Request retry
        }

        None
      }
    }
  }
}
