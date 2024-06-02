use std::collections::VecDeque;
use std::iter::Sum;
use num_traits::{Float, NumAssign};

/// Structure to calculate RMS in real time
pub struct RMS<T> {
  pub largest_window: usize,
  pub samples: VecDeque<T>
}

impl<T: Float + Sum + NumAssign> RMS<T> where f32: From<T> {
  /// Create a new RMS calculator
  pub fn new(largest_window: usize) -> Self {
    RMS {
      largest_window,
      samples: VecDeque::with_capacity(largest_window)
    }
  }

  /// Add a new sample to the RMS calculator
  pub fn add_sample(&mut self, sample: T) {
    if self.samples.len() == self.largest_window {
      self.samples.pop_front();
    }
    self.samples.push_back(sample * sample);
  }

  /// Calculate the current RMS value
  pub fn calculate_rms(&self, window: usize) -> f32 {
    assert!(window <= self.largest_window);
    if self.samples.is_empty() {
      0.0
    } else {
      (f32::from(self.samples.iter().rev().take(window).cloned().sum()) / window as f32).sqrt().into()
    }
  }

  /// Reset the RMS calculator
  pub fn reset(&mut self) {
    self.samples.clear();
  }
}
