use std::net::IpAddr;
use anyhow::{Result, Context};
use serde::{Serialize, Deserialize};

use super::{opcode::GatewayOpcode, GatewayPacket};

#[derive(Debug)]
pub enum GatewayEvent {
  Identify(Identify),
  SelectProtocol(SelectProtocol),
  Ready(Ready),
  Heartbeat(u64),
  SessionDescription(SessionDescription),
  Speaking(Speaking),
  HeartbeatAck(u64),
  Hello(Hello)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Identify {
  pub server_id: u64,
  pub user_id: u64,
  pub session_id: String,
  pub token: String
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SelectProtocol {
  pub protocol: String,
  pub data: SelectProtocolData
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SelectProtocolData {
  pub address: IpAddr,
  pub port: u16,
  pub mode: String
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Ready {
  pub ssrc: u32,
  pub ip: String,
  pub port: u16,
  pub modes: Vec<String>
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionDescription {
  pub mode: String,
  pub secret_key: Vec<u8>
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Speaking {
  pub speaking: u8,
  pub delay: u32,
  pub ssrc: u32
}

#[derive(Debug, Serialize, Deserialize)]
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
      Hello(_) => GatewayOpcode::Hello
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
    match packet.opcode {
      GatewayOpcode::Identify => Ok(GatewayEvent::Identify(serde_json::from_value(packet.data.context("no packet data")?)?)),
      GatewayOpcode::SelectProtocol => Ok(GatewayEvent::SelectProtocol(serde_json::from_value(packet.data.context("no packet data")?)?)),
      GatewayOpcode::Ready => Ok(GatewayEvent::Ready(serde_json::from_value(packet.data.context("no packet data")?)?)),
      GatewayOpcode::Heartbeat => Ok(GatewayEvent::Heartbeat(serde_json::from_value(packet.data.context("no packet data")?)?)),
      GatewayOpcode::SessionDescription => Ok(GatewayEvent::SessionDescription(serde_json::from_value(packet.data.context("no packet data")?)?)),
      GatewayOpcode::Speaking => Ok(GatewayEvent::Speaking(serde_json::from_value(packet.data.context("no packet data")?)?)),
      GatewayOpcode::HeartbeatAck => Ok(GatewayEvent::HeartbeatAck(serde_json::from_value(packet.data.context("no packet data")?)?)),
      GatewayOpcode::Hello => Ok(GatewayEvent::Hello(serde_json::from_value(packet.data.context("no packet data")?)?)),
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
        Hello(hello) => Some(serde_json::to_value(hello)?)
      }
    })
  }
}
