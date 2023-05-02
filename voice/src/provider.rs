/// Audio sample provider for [`VoiceConnection`](crate::VoiceConnection).
pub trait SampleProvider: Sync + Send {
  /// Returned samples are in 32-bit floating-point PCM format at 48 kHz sample rate.
  ///
  /// If no more samples are available, this function will return `0`.
  fn get_samples(&mut self, samples: &mut [f32]) -> usize;
}
