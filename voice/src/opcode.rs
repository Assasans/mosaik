use std::fmt;

use serde::{Deserialize, Serialize};

use self::GatewayOpcode::*;

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum GatewayOpcode {
  Identify,
  SelectProtocol,
  Ready,
  Heartbeat,
  SessionDescription,
  Speaking,
  HeartbeatAck,
  Resume,
  Hello,
  Resumed,
  ClientDisconnect,
  Unknown(u8)
}

impl fmt::Display for GatewayOpcode {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    let code: u8 = self.into();
    write!(f, "{}", code)
  }
}

impl From<GatewayOpcode> for u8 {
  fn from(code: GatewayOpcode) -> u8 {
    match code {
      Identify => 0,
      SelectProtocol => 1,
      Ready => 2,
      Heartbeat => 3,
      SessionDescription => 4,
      Speaking => 5,
      HeartbeatAck => 6,
      Resume => 7,
      Hello => 8,
      Resumed => 9,
      ClientDisconnect => 13,
      Unknown(code) => code
    }
  }
}

impl<'t> From<&'t GatewayOpcode> for u8 {
  fn from(code: &'t GatewayOpcode) -> u8 {
    (*code).into()
  }
}

impl From<u8> for GatewayOpcode {
  fn from(code: u8) -> GatewayOpcode {
    match code {
      0 => Identify,
      1 => SelectProtocol,
      2 => Ready,
      3 => Heartbeat,
      4 => SessionDescription,
      5 => Speaking,
      6 => HeartbeatAck,
      7 => Resume,
      8 => Hello,
      9 => Resumed,
      13 => ClientDisconnect,
      _ => Unknown(code)
    }
  }
}

impl Serialize for GatewayOpcode {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: serde::Serializer
  {
    serializer.serialize_u8((*self).into())
  }
}

impl<'de> Deserialize<'de> for GatewayOpcode {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: serde::Deserializer<'de>
  {
    let value = u8::deserialize(deserializer)?;
    Ok(value.into())
  }
}
