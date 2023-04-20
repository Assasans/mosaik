use std::time::{Instant, SystemTime};

use anyhow::{Result, Context, anyhow};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{WebSocketStream, MaybeTlsStream, connect_async, tungstenite::Message};
use tracing::{debug, warn};

use super::{Hello, Ready, VoiceConnectionOptions, Speaking, GatewayPacket, GatewayEvent, Identify};

#[derive(Debug)]
pub struct WebSocketVoiceConnection {
  pub socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
  pub heartbeat_time: Instant,

  pub hello: Option<Hello>,
  pub ready: Option<Ready>
}

impl WebSocketVoiceConnection {
  pub async fn new(options: VoiceConnectionOptions) -> Result<Self> {
    let (socket, _) = connect_async(format!("wss://{}/?v=4", options.endpoint)).await?;
    debug!("voice gateway connected");

    let mut me = Self {
      socket,
      heartbeat_time: Instant::now(),

      hello: None,
      ready: None
    };

    me.send(GatewayEvent::Identify(Identify {
      server_id: options.guild_id,
      user_id: options.user_id,
      session_id: options.session_id,
      token: options.token
    }).try_into()?).await?;

    let mut hello = None;
    let mut ready = None;
    loop {
      let event: GatewayEvent = me.receive().await?.try_into()?;
      match event {
        GatewayEvent::Ready(it) => {
          ready = Some(it);
          if hello.is_some() {
            break;
          }
        },
        GatewayEvent::Hello(it) => {
          hello = Some(it);
          if ready.is_some() {
            break;
          }
        },
        other => {
          warn!("Expected Ready / Hello packet, got: {:?}", other);
          return Err(anyhow!("Invalid packet")); // TODO
        }
      }
    }

    me.hello = Some(hello.unwrap());
    me.ready = Some(ready.unwrap());

    Ok(me)
  }

  pub async fn send_speaking(&mut self, speaking: bool) -> Result<()> {
    let ready = self.ready.as_ref().context("no voice ready packet")?;

    self.send(GatewayEvent::Speaking(Speaking {
      speaking: if speaking { 1 } else { 0 },
      delay: 0,
      ssrc: ready.ssrc
    }).try_into()?).await?;

    Ok(())
  }

  pub async fn send_heartbeat(&mut self) -> Result<()> {
    let nonce = u64::try_from(SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?.as_millis())?;
    self.send(GatewayEvent::Heartbeat(nonce).try_into()?).await?;

    let ack_nonce = {
      let event: GatewayEvent = self.receive().await?.try_into()?;
      match event {
        GatewayEvent::HeartbeatAck(nonce) => nonce,
        other => {
          warn!("Expected HeartbeatAck packet, got: {:?}", other);
          return Err(anyhow!("Invalid packet")); // TODO
        }
      }
    };

    debug!("Sent gateway heartbeat, ack nonce: {}", ack_nonce);

    self.heartbeat_time = Instant::now();

    Ok(())
  }

  pub async fn send(&mut self, packet: GatewayPacket) -> Result<()> {
    let json = serde_json::to_string(&packet)?;
    debug!("> {}", json);

    self.socket.send(Message::Text(json)).await?;
    self.socket.flush().await?;

    Ok(())
  }

  pub async fn receive(&mut self) -> Result<GatewayPacket> {
    let message = self.socket.next().await.context("no next voice gateway message")??;
    let json = message.into_text()?;
    debug!("< {}", json);

    Ok(serde_json::from_str(&json)?)
  }
}
