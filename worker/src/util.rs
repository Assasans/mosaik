use thiserror::Error;

#[derive(Debug, Error)]
#[error("Enum variant mismatch")]
pub struct MismatchError;

#[macro_export]
macro_rules! include_and_export {
  ($($module:ident)+) => {
    $(
      mod $module;
      pub use $module::*;
    )*
  };
}

#[macro_export]
macro_rules! try_unpack {
  ($value:expr, $variant:path) => {{
    match $value {
      $variant(x) => Ok(x),
      _ => Err($crate::util::MismatchError)
    }
  }};
}

#[macro_export]
macro_rules! interaction_response {
  ($type:ident $(, $method:ident ( $( $arg:expr ),* ))*) => {{
    use ::twilight_model::http::interaction::{InteractionResponse, InteractionResponseType};
    use ::twilight_util::builder::InteractionResponseDataBuilder;

    let mut builder = InteractionResponseDataBuilder::new();
    $(builder = builder.$method($($arg),*);)*

    InteractionResponse {
      kind: InteractionResponseType::$type,
      data: Some(builder.build())
    }
  }}
}

#[macro_export]
macro_rules! get_option {
  ($command:expr, $name:expr) => {
    $command.options.iter().find(|it| it.name == $name).map(|it| it.value)
  };
}

#[macro_export]
macro_rules! get_option_as {
  ($command:expr, $name:expr, $type:path) => {{
    let value = $command.options.iter().find(|it| it.name == $name).map(|it| &it.value);
    value.map(|it| try_unpack!(it, $type))
  }};
}

#[macro_export]
macro_rules! reply {
  ($state:expr, $interaction:expr, $response:expr) => {
    $state
      .http
      .interaction($state.application_id)
      .create_response($interaction.id, &$interaction.token, $response)
  };
}

#[macro_export]
macro_rules! update_reply {
  ($state:expr, $interaction:expr) => {
    $state
      .http
      .interaction($state.application_id)
      .update_response(&$interaction.token)
  };
}
