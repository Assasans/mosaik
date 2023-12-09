use serenity::all::UserId;
use crate::providers::MediaProvider;

#[derive(Debug)]
pub struct Track {
  pub provider: Box<dyn MediaProvider>,
  pub creator: Option<UserId>
}

impl Track {
  pub fn new(provider: Box<dyn MediaProvider>, creator: Option<UserId>) -> Self {
    Self {
      provider,
      creator
    }
  }
}
