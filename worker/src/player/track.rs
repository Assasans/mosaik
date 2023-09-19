use twilight_model::id::Id;
use twilight_model::id::marker::UserMarker;

use crate::providers::MediaProvider;

#[derive(Debug)]
pub struct Track {
  pub provider: Box<dyn MediaProvider>,
  pub creator: Option<Id<UserMarker>>
}

impl Track {
  pub fn new(provider: Box<dyn MediaProvider>, creator: Option<Id<UserMarker>>) -> Self {
    Self {
      provider,
      creator
    }
  }
}
