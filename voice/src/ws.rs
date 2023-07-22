use std::time::SystemTime;
use anyhow::{Result, Context, anyhow};
use flume::{Receiver, Sender};
use futures_util::{SinkExt, StreamExt};
use tokio::select;
use tokio_tungstenite::{connect_async, tungstenite::{Message, protocol::CloseFrame}};
use tracing::{debug, warn};
use crate::Resume;

use super::{Hello, Ready, VoiceConnectionOptions, Speaking, GatewayPacket, GatewayEvent, Identify};

pub struct WebSocketVoiceConnection {
  pub read: Receiver<GatewayPacket>,
  write: Sender<GatewayPacket>,
  close_tx: Sender<CloseFrame<'static>>,
  pub close_rx: Receiver<Option<CloseFrame<'static>>>,

  pub options: VoiceConnectionOptions,
  pub hello: Option<Hello>,
  pub ready: Option<Ready>
}

pub enum VoiceConnectionMode {
  New(VoiceConnectionOptions),
  Resume { options: VoiceConnectionOptions, hello: Hello, ready: Ready }
}

impl WebSocketVoiceConnection {
  pub async fn new(mode: VoiceConnectionMode) -> Result<Self> {
    let options = match &mode {
      VoiceConnectionMode::New(options) => options,
      VoiceConnectionMode::Resume { options, .. } => options
    };

    let (mut socket, _) = connect_async(format!("wss://{}/?v=4", options.endpoint)).await?;
    debug!("voice gateway connected");

    let (read_tx, read_rx) = flume::unbounded();
    let (write_tx, write_rx) = flume::unbounded();
    let (close_tx_tx, close_tx_rx) = flume::bounded(0);
    let (close_rx_tx, close_rx_rx) = flume::unbounded();

    // WebSocket IO task
    tokio::spawn(async move {
      loop {
        select! {
          message = socket.next() => {
            match message {
              Some(message) => {
                let message = message.unwrap();
                match message {
                  Message::Text(json) => {
                    debug!("< {}", json);
                    read_tx.send_async(serde_json::from_str(&json).unwrap()).await.unwrap();
                  }

                  Message::Close(frame) => {
                    debug!("voice gateway closed with {:?}", frame);
                    close_rx_tx.send_async(frame);
                  }

                  _ => {
                    warn!("unknown voice gateway frame {:?}", message);
                  }
                }
              },
              None => break
            }
          }

          packet = write_rx.recv_async() => {
            let packet = packet.unwrap();

            let json = serde_json::to_string(&packet).unwrap();
            debug!("> {}", json);

            socket.send(Message::Text(json)).await.unwrap();
            socket.flush().await.unwrap();
          }

          frame = close_tx_rx.recv_async() => {
            let frame = frame.unwrap();
            socket.close(Some(frame)).await.unwrap();
          }
        }
      }
    });

    let mut me = Self {
      read: read_rx,
      write: write_tx,
      close_tx: close_tx_tx,
      close_rx: close_rx_rx,

      options: options.to_owned(),
      hello: None,
      ready: None
    };

    match mode {
      VoiceConnectionMode::New(_) => {
        me.send_identify().await?;

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
              warn!("Expected Ready or Hello packet, got: {:?}", other);
              return Err(anyhow!("Invalid packet")); // TODO
            }
          }
        }

        me.hello = Some(hello.unwrap());
        me.ready = Some(ready.unwrap());
      }

      VoiceConnectionMode::Resume { hello, ready, .. } => {
        me.hello = Some(hello);
        me.ready = Some(ready);
        me.send_resume().await?;
      }
    }

    Ok(me)
  }

  pub async fn send_speaking(&self, speaking: bool) -> Result<()> {
    let ready = self.ready.as_ref().context("no voice ready packet")?;

    self.send(GatewayEvent::Speaking(Speaking {
      speaking: if speaking { 1 } else { 0 },
      delay: 0,
      ssrc: ready.ssrc
    }).try_into()?).await?;

    Ok(())
  }

  pub async fn send_identify(&self) -> Result<()> {
    self.send(GatewayEvent::Identify(Identify {
      server_id: self.options.guild_id,
      user_id: self.options.user_id,
      session_id: self.options.session_id.to_owned(),
      token: self.options.token.to_owned()
    }).try_into()?).await?;
    Ok(())
  }

  pub async fn send_resume(&self) -> Result<()> {
    self.send(GatewayEvent::Resume(Resume {
      server_id: self.options.guild_id,
      session_id: self.options.session_id.to_owned(),
      token: self.options.token.to_owned()
    }).try_into()?).await?;
    Ok(())
  }

  pub async fn send_heartbeat(&self) -> Result<()> {
    let nonce = u64::try_from(SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?.as_millis())?;

    self.send(GatewayEvent::Heartbeat(nonce).try_into()?).await?;
    debug!("Sent gateway heartbeat");

    Ok(())
  }

  pub async fn send(&self, packet: GatewayPacket) -> Result<()> {
    self.write.send_async(packet).await?;
    Ok(())
  }

  pub async fn receive(&self) -> Result<GatewayPacket> {
    Ok(self.read.recv_async().await?)
  }

  pub async fn close(&self, frame: CloseFrame<'_>) -> Result<()> {
    self.close_tx.send_async(frame.into_owned()).await?;
    Ok(())
  }
}
