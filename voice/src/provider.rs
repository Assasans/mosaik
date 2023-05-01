pub trait SampleProvider: Sync + Send {
  fn get_samples(&mut self, samples: &mut [f32]) -> usize;
}
