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

macro_rules! metadata {
  ($($kind:ident => $block:block),*$(,)?) => {{
    let mut metadata = Vec::new();
    $(
      if let Some(value) = $block {
        metadata.push(MediaMetadata::$kind(value.to_owned()));
      }
    )*
    metadata
  }};
}

pub(crate) use metadata;

// TODO(Assasans): Generalize for non-metadata enums
macro_rules! get_metadata {
  ($metadata:expr, $matcher:pat => $result:expr) => {
    $metadata.iter().find_map(|item| match item {
      $matcher => Some($result),
      _ => None
    })
  };
}
