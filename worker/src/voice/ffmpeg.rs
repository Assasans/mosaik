use decoder::Decoder;
use voice::provider::SampleProvider;

pub struct FFmpegSampleProvider {
  pub decoder: Decoder
}

impl FFmpegSampleProvider {
  pub fn new() -> Self {
    Self {
      decoder: Decoder::new()
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
  fn get_samples(&mut self, samples: &mut [f32]) -> usize {
    let read = self.decoder.read_frame();
    samples[..read.len()].copy_from_slice(&read);
    read.len()
  }
}
