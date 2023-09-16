use std::any::Any;

/// Audio sample provider for [`VoiceConnection`](crate::VoiceConnection).
pub trait SampleProvider: Sync + Send {
  /// The provided samples are returned in 32-bit floating point PCM format and have a sampling rate of 48 kHz.
  ///
  /// If there are no additional samples available at the moment, this function will return [`None`].
  /// If there are no samples currently available but could potentially become available later, this function returns an empty vector.
  fn get_samples(&mut self) -> Option<Vec<f32>>;

  fn as_any(&mut self) -> &mut (dyn Any + Sync + Send);

  fn get_handle(&self) -> Box<dyn SampleProviderHandle>;
}

/// Audio sample provider handle for [`SampleProvider`].
///
/// Used to communicate with a locked [`SampleProvider`] during playback.
pub trait SampleProviderHandle: Sync + Send {
  fn as_any(&self) -> &(dyn Any + Sync + Send);
}
