use std::time::Duration;

#[derive(Debug)]
pub enum MediaMetadata {
  Id(String),
  Title(String),
  Url(String),
  Thumbnail(String),
  Description(String),
  Duration(Duration),
  ViewCount(u64)
}
