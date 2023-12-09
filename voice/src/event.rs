use std::net::IpAddr;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::opcode::GatewayOpcode;
use super::GatewayPacket;

#[derive(Clone, Debug)]
pub enum GatewayEvent {
  Identify(Identify),
  SelectProtocol(SelectProtocol),
  Ready(Ready),
  Heartbeat(u64),
  SessionDescription(SessionDescription),
  Speaking(Speaking),
  HeartbeatAck(u64),
  Resume(Resume),
  Hello(Hello),
  Resumed
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Identify {
  pub server_id: u64,
  pub user_id: u64,
  pub session_id: String,
  pub token: String
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectProtocol {
  pub protocol: String,
  pub data: SelectProtocolData
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectProtocolData {
  pub address: IpAddr,
  pub port: u16,
  pub mode: String
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ready {
  pub ssrc: u32,
  pub ip: String,
  pub port: u16,
  pub modes: Vec<String>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionDescription {
  pub mode: String,
  pub secret_key: Vec<u8>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Speaking {
  pub speaking: u8,
  pub delay: u32,
  pub ssrc: u32
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Resume {
  pub server_id: u64,
  pub session_id: String,
  pub token: String
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hello {
  pub heartbeat_interval: f32
}

impl From<&GatewayEvent> for GatewayOpcode {
  fn from(event: &GatewayEvent) -> GatewayOpcode {
    use GatewayEvent::*;
    match event {
      Identify(_) => GatewayOpcode::Identify,
      SelectProtocol(_) => GatewayOpcode::SelectProtocol,
      Ready(_) => GatewayOpcode::Ready,
      Heartbeat(_) => GatewayOpcode::Heartbeat,
      SessionDescription(_) => GatewayOpcode::SessionDescription,
      Speaking(_) => GatewayOpcode::Speaking,
      HeartbeatAck(_) => GatewayOpcode::HeartbeatAck,
      Resume(_) => GatewayOpcode::Resume,
      Hello(_) => GatewayOpcode::Hello,
      Resumed => GatewayOpcode::Resumed
    }
  }
}

impl From<GatewayEvent> for GatewayOpcode {
  fn from(event: GatewayEvent) -> GatewayOpcode {
    (&event).into()
  }
}

impl TryFrom<GatewayPacket> for GatewayEvent {
  type Error = anyhow::Error; // TODO

  fn try_from(packet: GatewayPacket) -> Result<GatewayEvent, Self::Error> {
    use serde_json::from_value;
    use GatewayOpcode::*;

    let data = packet.data.context("no packet data");
    match packet.opcode {
      Identify => Ok(GatewayEvent::Identify(from_value(data?)?)),
      SelectProtocol => Ok(GatewayEvent::SelectProtocol(from_value(data?)?)),
      Ready => Ok(GatewayEvent::Ready(from_value(data?)?)),
      Heartbeat => Ok(GatewayEvent::Heartbeat(from_value(data?)?)),
      SessionDescription => Ok(GatewayEvent::SessionDescription(from_value(data?)?)),
      Speaking => Ok(GatewayEvent::Speaking(from_value(data?)?)),
      HeartbeatAck => Ok(GatewayEvent::HeartbeatAck(from_value(data?)?)),
      Resume => Ok(GatewayEvent::Resume(from_value(data?)?)),
      Hello => Ok(GatewayEvent::Hello(from_value(data?)?)),
      Resumed => Ok(GatewayEvent::Resumed),
      _ => Err(anyhow::anyhow!("Unsupported opcode: {}", packet.opcode))
    }
  }
}

impl TryFrom<GatewayEvent> for GatewayPacket {
  type Error = anyhow::Error; // TODO

  fn try_from(event: GatewayEvent) -> Result<GatewayPacket, Self::Error> {
    use GatewayEvent::*;
    Ok(GatewayPacket {
      opcode: (&event).into(),
      data: match event {
        Identify(identify) => Some(serde_json::to_value(identify)?),
        SelectProtocol(select_protocol) => Some(serde_json::to_value(select_protocol)?),
        Ready(ready) => Some(serde_json::to_value(ready)?),
        Heartbeat(nonce) => Some(serde_json::to_value(nonce)?),
        SessionDescription(session_description) => Some(serde_json::to_value(session_description)?),
        Speaking(speaking) => Some(serde_json::to_value(speaking)?),
        HeartbeatAck(nonce) => Some(serde_json::to_value(nonce)?),
        Resume(resume) => Some(serde_json::to_value(resume)?),
        Hello(hello) => Some(serde_json::to_value(hello)?),
        Resumed => None
      }
    })
  }
}
