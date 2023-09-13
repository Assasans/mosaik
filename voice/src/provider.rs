/// Audio sample provider for [`VoiceConnection`](crate::VoiceConnection).
pub trait SampleProvider: Sync + Send {
  /// The provided samples are returned in 32-bit floating point PCM format and have a sampling rate of 48 kHz.
  ///
  /// If there are no additional samples available at the moment, this function will return [`None`].
  /// If there are no samples currently available but could potentially become available later, this function returns `0`.
  fn get_samples(&mut self, samples: &mut [f32]) -> Option<usize>;
}
