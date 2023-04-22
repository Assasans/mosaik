use std::{time::{SystemTime, Duration}, sync::Arc};
use anyhow::{Result, Context, anyhow};
use async_channel::Receiver;
use futures::stream::{SplitStream, SplitSink};
use futures_util::{SinkExt, StreamExt};
use tokio::{net::TcpStream, time::{Interval, interval}, sync::Mutex};
use tokio_tungstenite::{WebSocketStream, MaybeTlsStream, connect_async, tungstenite::{Message, protocol::CloseFrame}};
use tracing::{debug, warn};

use super::{Hello, Ready, VoiceConnectionOptions, Speaking, GatewayPacket, GatewayEvent, Identify};

pub struct WebSocketVoiceConnection {
  pub read: Arc<Mutex<Option<SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>>>>,
  pub write: Arc<Mutex<Option<SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>>>>,

  pub packets: Receiver<GatewayPacket>,

  pub hello: Option<Hello>,
  pub ready: Option<Ready>
}

impl WebSocketVoiceConnection {
  pub async fn new(options: VoiceConnectionOptions) -> Result<Self> {
    let (socket, _) = connect_async(format!("wss://{}/?v=4", options.endpoint)).await?;
    debug!("voice gateway connected");

    let (sender, receiver) = async_channel::unbounded();
    let (write, read) = socket.split();

    let read = Arc::new(Mutex::new(Some(read)));

    let read_weak = Arc::downgrade(&read);
    tokio::spawn(async move {
      while let Some(read) = read_weak.upgrade() {
        let mut lock = read.try_lock().unwrap();
        match lock.as_mut().unwrap().next().await {
          Some(message) => {
            let message = message.unwrap();
            let json = message.into_text().unwrap();
            debug!("< {}", json);

            sender.send(serde_json::from_str(&json).unwrap()).await.unwrap();
          },
          None => break
        }
      }
    });

    let mut me = Self {
      read: read,
      write: Arc::new(Mutex::new(Some(write))),

      packets: receiver,

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

  pub async fn send_speaking(&self, speaking: bool) -> Result<()> {
    let ready = self.ready.as_ref().context("no voice ready packet")?;

    self.send(GatewayEvent::Speaking(Speaking {
      speaking: if speaking { 1 } else { 0 },
      delay: 0,
      ssrc: ready.ssrc
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
    let json = serde_json::to_string(&packet)?;
    debug!("> {}", json);

    let mut lock = self.write.lock().await;
    let write = lock.as_mut().unwrap();

    write.send(Message::Text(json)).await?;
    write.flush().await?;

    Ok(())
  }

  pub async fn receive(&self) -> Result<GatewayPacket> {
    Ok(self.packets.recv().await?)
  }

  pub async fn close(&self, frame: CloseFrame<'_>) -> Result<()> {
    let read = self.read.lock().await.take().unwrap();
    let write = self.write.lock().await.take().unwrap();

    let mut socket = write.reunite(read)?;
    socket.close(Some(frame)).await?;

    Ok(())
  }
}
