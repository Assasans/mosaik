use regex::Regex;

pub struct MediaProviderPredictor {}

impl MediaProviderPredictor {
  pub fn new() -> Self {
    MediaProviderPredictor {}
  }

  pub fn predict(&self, query: &str) -> Vec<PredictionResult> {
    if Regex::new(r"(youtube\.com|youtu\.be)").unwrap().is_match(query) {
      if Regex::new(r"[?&]list=([^#?&]*)").unwrap().is_match(query) {
        return vec![PredictionResult::new(0.9, PredictedProvider::YtDlpPlaylist)];
      }

      if Regex::new(r"https?://(?:www\.)?youtu(?:be\.com/watch\?v=|\.be/)([\w\-_]+)").unwrap().is_match(query) {
        return vec![PredictionResult::new(0.9, PredictedProvider::YtDlp)];
      }
    }
    vec![]
  }
}

#[derive(Debug)]
pub enum PredictedProvider {
  FFmpeg,
  YtDlp,
  YtDlpPlaylist,
}

#[derive(Debug)]
pub struct PredictionResult {
  pub score: f32,
  pub provider: PredictedProvider,
}

impl PredictionResult {
  pub fn new(score: f32, provider: PredictedProvider) -> Self {
    PredictionResult { score, provider }
  }
}
