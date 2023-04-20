use std::time::Instant;

use anyhow::Result;
use discortp::{wrap::{Wrap16, Wrap32}, discord::MutableKeepalivePacket};
use rand::random;
use tokio::net::UdpSocket;
use tracing::debug;

use super::Ready;

#[derive(Debug)]
pub struct UdpVoiceConnection {
  pub socket: UdpSocket,
  pub heartbeat_time: Instant,

  pub sequence: Wrap16,
  pub timestamp: Wrap32,
  pub deadline: Instant,

  pub rtp_buffer: Vec<u8>
}

impl UdpVoiceConnection {
  pub async fn new(ready: &Ready) -> Result<Self> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.connect((ready.ip.clone(), ready.port)).await?;

    Ok(Self {
      socket,
      sequence: random::<u16>().into(),
      timestamp: random::<u32>().into(),
      heartbeat_time: Instant::now(),
      deadline: Instant::now(),

      rtp_buffer: vec![0; 1460]
    })
  }

  pub async fn send_keepalive(&mut self, ready: &Ready) -> Result<()> {
    let mut buffer = [0; MutableKeepalivePacket::minimum_packet_size()];
    let mut view = MutableKeepalivePacket::new(&mut buffer).unwrap();
    view.set_ssrc(ready.ssrc);

    self.heartbeat_time = Instant::now();
    self.socket.send(&buffer).await?;
    debug!("Sent UDP keepalive");

    Ok(())
  }
}
