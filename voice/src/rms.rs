use std::collections::VecDeque;
use std::iter::Sum;
use num_traits::{Float, NumAssign};

/// Structure to calculate RMS in real time
pub struct RMS<T> {
  pub samples: VecDeque<T>,
  pub window_size: usize
}

impl<T: Float + Sum + NumAssign> RMS<T> where f32: From<T> {
  /// Create a new RMS calculator
  pub fn new(window_size: usize) -> Self {
    RMS {
      samples: VecDeque::with_capacity(window_size),
      window_size
    }
  }

  /// Add a new sample to the RMS calculator
  pub fn add_sample(&mut self, sample: T) {
    if self.samples.len() == self.window_size {
      self.samples.pop_front();
    }
    self.samples.push_back(sample * sample);
  }

  /// Calculate the current RMS value
  pub fn calculate_rms(&self) -> f32 {
    if self.samples.is_empty() {
      0.0
    } else {
      (f32::from(self.samples.iter().cloned().sum()) / self.samples.len() as f32).sqrt().into()
    }
  }

  /// Reset the RMS calculator
  pub fn reset(&mut self) {
    self.samples.clear();
  }
}
