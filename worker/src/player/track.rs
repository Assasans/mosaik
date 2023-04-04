use twilight_model::id::{Id, marker::{GuildMarker, ChannelMarker}};

use crate::providers::MediaProvider;

#[derive(Debug)]
pub struct Track {
  pub provider: Box<dyn MediaProvider>
}

impl Track {
  pub fn new(provider: Box<dyn MediaProvider>) -> Self {
    Self {
      provider
    }
  }
}
