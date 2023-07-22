use std::fmt;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;

use self::GatewayCloseCode::*;

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum GatewayCloseCode {
  UnknownOpcode,
  FailedToDecodePayload,
  NotAuthenticated,
  AuthenticationFailed,
  AlreadyAuthenticated,
  SessionNoLongerValid,
  SessionTimeout,
  ServerNotFound,
  UnknownProtocol,
  Disconnected,
  VoiceServerCrashed,
  UnknownEncryptionMode,
  Unknown(u16)
}

impl GatewayCloseCode {
  pub fn can_reconnect(self) -> bool {
    matches!(
      self,
      VoiceServerCrashed
    )
  }
}

impl fmt::Display for GatewayCloseCode {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    let code: u16 = self.into();
    write!(f, "{}", code)
  }
}

impl From<GatewayCloseCode> for u16 {
  fn from(code: GatewayCloseCode) -> u16 {
    match code {
      UnknownOpcode => 4001,
      FailedToDecodePayload => 4002,
      NotAuthenticated => 4003,
      AuthenticationFailed => 4004,
      AlreadyAuthenticated => 4005,
      SessionNoLongerValid => 4006,
      SessionTimeout => 4009,
      ServerNotFound => 4011,
      UnknownProtocol => 4012,
      Disconnected => 4014,
      VoiceServerCrashed => 4015,
      UnknownEncryptionMode => 4016,
      Unknown(code) => code
    }
  }
}

impl<'t> From<&'t GatewayCloseCode> for u16 {
  fn from(code: &'t GatewayCloseCode) -> u16 {
    (*code).into()
  }
}

impl From<u16> for GatewayCloseCode {
  fn from(code: u16) -> GatewayCloseCode {
    match code {
      4001 => UnknownOpcode,
      4002 => FailedToDecodePayload,
      4003 => NotAuthenticated,
      4004 => AuthenticationFailed,
      4005 => AlreadyAuthenticated,
      4006 => SessionNoLongerValid,
      4009 => SessionTimeout,
      4011 => ServerNotFound,
      4012 => UnknownProtocol,
      4014 => Disconnected,
      4015 => VoiceServerCrashed,
      4016 => UnknownEncryptionMode,
      _ => Unknown(code)
    }
  }
}

impl From<CloseCode> for GatewayCloseCode {
  fn from(code: CloseCode) -> GatewayCloseCode {
    Into::<u16>::into(code).into()
  }
}
