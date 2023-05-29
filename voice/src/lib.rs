pub mod constants;
pub mod opcode;
pub mod event;
pub mod provider;
pub mod ws;
pub mod udp;

use tokio::{sync::Mutex, select, time::{interval, Interval}};
use tracing::*;
use std::{
  fmt::Debug,
  net::IpAddr,
  str::FromStr,
  sync::{Arc, Weak},
  time::{Duration, Instant}
};
use opus::{Encoder, Bitrate, Channels, Application};
use anyhow::{Result, anyhow, Context};
use discortp::{
  MutablePacket,
  discord::{IpDiscoveryPacket, MutableIpDiscoveryPacket, IpDiscoveryType},
  rtp::{MutableRtpPacket, RtpType}
};
use rand::random;
use ringbuf::HeapRb;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_tungstenite::{tungstenite::protocol::{CloseFrame, frame::coding::CloseCode}};
use xsalsa20poly1305::{aead::generic_array::GenericArray, TAG_SIZE, XSalsa20Poly1305, KeyInit, Key, AeadInPlace};

pub use opcode::*;
pub use event::*;

use utils::state_flow::StateFlow;
use self::{
  constants::{SAMPLE_RATE, CHANNEL_COUNT, CHUNK_DURATION, TIMESTAMP_STEP},
  provider::SampleProvider,
  ws::WebSocketVoiceConnection,
  udp::UdpVoiceConnection
};

#[derive(Debug, Serialize, Deserialize)]
pub struct GatewayPacket {
  #[serde(rename = "op")]
  opcode: GatewayOpcode,
  #[serde(rename = "d")]
  data: Option<Value>
}

impl GatewayPacket {
  pub fn new<T>(opcode: GatewayOpcode, data: T) -> Self where T: Into<Option<Value>> {
    Self {
      opcode,
      data: data.into()
    }
  }
}

#[derive(Debug, Eq, PartialEq)]
#[non_exhaustive]
enum VoiceCipherMode {
  Normal,
  Suffix,
  Lite
}

#[derive(Debug, Clone)]
pub struct VoiceConnectionOptions {
  pub user_id: u64,
  pub guild_id: u64,
  pub bitrate: Option<u32>,

  pub endpoint: String,
  pub token: String,
  pub session_id: String
}

struct IpDiscoveryResult {
  pub address: IpAddr,
  pub port: u16
}

pub enum VoiceConnectionState {
  Disconnected,
  Connected,
  Playing
}

pub struct VoiceConnection {
  pub ws: Mutex<Option<WebSocketVoiceConnection>>,
  ws_heartbeat_interval: Mutex<Option<Interval>>,
  pub udp: Mutex<Option<UdpVoiceConnection>>,
  cipher: Mutex<Option<XSalsa20Poly1305>>,
  cipher_mode: VoiceCipherMode,
  opus_encoder: Mutex<Encoder>,
  pub sample_provider: Mutex<Option<Box<dyn SampleProvider>>>,
  pub state: StateFlow<VoiceConnectionState>
}

impl VoiceConnection {
  pub fn new() -> Result<Self> {
    Ok(Self {
      ws: Mutex::new(None),
      ws_heartbeat_interval: Mutex::new(None),
      udp: Mutex::new(None),
      cipher: Mutex::new(None),
      cipher_mode: VoiceCipherMode::Suffix,
      opus_encoder: Mutex::new(Encoder::new(48000, Channels::Stereo, Application::Audio)?),
      sample_provider: Mutex::new(None),
      state: StateFlow::new(VoiceConnectionState::Disconnected)
    })
  }

  pub async fn connect(&self, options: VoiceConnectionOptions) -> Result<()> {
    if let Some(bitrate) = options.bitrate {
      self.opus_encoder.lock().await.set_bitrate(Bitrate::Bits(i32::try_from(bitrate)?))?;
    }

    *self.ws.lock().await = Some(WebSocketVoiceConnection::new(options.clone()).await?);

    let mut ws_guard = self.ws.lock().await;
    let ws = ws_guard.as_mut().context("no voice gateway connection")?;

    let hello = ws.hello.as_ref().context("no voice hello packet")?;
    let ready = ws.ready.as_ref().context("no voice ready packet")?;

    *self.ws_heartbeat_interval.lock().await = Some(interval(Duration::from_millis(hello.heartbeat_interval.round() as u64)));

    *self.udp.lock().await = Some(UdpVoiceConnection::new(ready).await?);

    let ip = self.discover_udp_ip(ready).await?;

    ws.send(GatewayEvent::SelectProtocol(SelectProtocol {
      protocol: "udp".to_owned(),
      data: SelectProtocolData {
        address: ip.address,
        port: ip.port,
        mode: "xsalsa20_poly1305_suffix".to_owned()
      }
    }).try_into()?).await?;

    let session_description = loop {
      // Ignore undocumented opcode 18
      let event: GatewayEvent = match ws.receive().await?.try_into() {
        Ok(event) => event,
        Err(_) => continue
      };

      match event {
        GatewayEvent::SessionDescription(description) => break description,
        other => {
          warn!("Expected SessionDescription packet, got: {:?}", other);
          return Err(anyhow!("Invalid packet")); // TODO
        }
      }
    };

    let key = Key::from_slice(&session_description.secret_key);
    *self.cipher.lock().await = Some(XSalsa20Poly1305::new(&key));

    self.state.set(VoiceConnectionState::Connected).await?;

    Ok(())
  }

  pub async fn disconnect(&self) -> Result<()> {
    self.state.set(VoiceConnectionState::Disconnected).await?;
    *self.udp.lock().await = None;

    let mut ws_lock = self.ws.lock().await;
    if let Some(ref mut ws) = *ws_lock {
      ws.close(CloseFrame {
        code: CloseCode::Normal,
        reason: "".into()
      }).await?;
      *ws_lock = None;
    }

    self.opus_encoder.lock().await.reset_state()?;

    Ok(())
  }

  async fn discover_udp_ip(&self, ready: &Ready) -> Result<IpDiscoveryResult> {
    let mut udp_guard = self.udp.lock().await;
    let udp = udp_guard.as_mut().context("no voice UDP socket")?;

    let mut buffer = [0; IpDiscoveryPacket::const_packet_size()];
    let mut view = MutableIpDiscoveryPacket::new(&mut buffer).unwrap();
    view.set_pkt_type(IpDiscoveryType::Request);
    view.set_length(70);
    view.set_ssrc(ready.ssrc);
    udp.socket.send(&buffer).await?;

    let (length, _address) = udp.socket.recv_from(&mut buffer).await?;
    let view = IpDiscoveryPacket::new(&buffer[..length]).unwrap();
    if view.get_pkt_type() != IpDiscoveryType::Response {
      return Err(anyhow!("Invalid response")); // TODO
    }

    let null_index = view
      .get_address_raw()
      .iter()
      .position(|&b| b == 0)
      .unwrap();

    Ok(IpDiscoveryResult {
      address: std::str::from_utf8(&view.get_address_raw()[..null_index]).map(|it| IpAddr::from_str(it))??,
      port: view.get_port()
    })
  }

  pub async fn send_voice_packet(&self, ready: &Ready, udp: &mut UdpVoiceConnection, input: &[f32]) -> Result<()> {
    let cipher_guard = self.cipher.lock().await;
    let cipher = cipher_guard.as_ref().context("no voice cipher")?;

    let rtp_buffer_length = udp.rtp_buffer.len();
    let mut view = MutableRtpPacket::new(&mut *udp.rtp_buffer).unwrap();
    view.set_version(2);
    view.set_payload_type(RtpType::Unassigned(0x78));

    view.set_sequence(udp.sequence);
    udp.sequence += 1;

    view.set_timestamp(udp.timestamp);
    udp.timestamp += TIMESTAMP_STEP as u32;

    view.set_ssrc(ready.ssrc);

    let payload = view.payload_mut();

    assert_eq!(self.cipher_mode, VoiceCipherMode::Suffix); // TODO: Implement rest
    let nonce_bytes = random::<[u8; 24]>();
    let nonce = GenericArray::from_slice(&nonce_bytes);

    let size = self.opus_encoder.lock().await.encode_float(input, &mut payload[TAG_SIZE..TAG_SIZE + rtp_buffer_length - 12 - nonce_bytes.len()])?;

    payload[TAG_SIZE + size..TAG_SIZE + size + nonce_bytes.len()].copy_from_slice(&nonce_bytes);

    let tag = cipher.encrypt_in_place_detached(
      nonce,
      b"",
      &mut payload[TAG_SIZE..TAG_SIZE + size]
    );
    match tag {
      Ok(tag) => {
        payload[..TAG_SIZE].copy_from_slice(tag.as_slice());

        spin_sleep::sleep(udp.deadline - Instant::now());
        let delta = Instant::now().saturating_duration_since(udp.deadline);
        udp.deadline = Instant::now() + CHUNK_DURATION;

        if delta > CHUNK_DURATION {
          warn!("Voice packet deadline exceeded by {:?}", delta);
        }

        udp.socket.send(&udp.rtp_buffer[..12 + TAG_SIZE + size + nonce_bytes.len()]).await?;
      },
      Err(error) => {
        return Err(anyhow!(error));
      }
    }

    Ok(())
  }

  pub async fn run_ws_loop(me: Weak<Self>) -> Result<()> {
    let packets = {
      let me = me.upgrade().context("voice connection dropped")?;
      let mut ws_guard = me.ws.lock().await;
      let ws = ws_guard.as_mut().context("no voice gateway connection")?;

      ws.packets.clone()
    };

    while let Some(me) = me.upgrade() {
      let mut interval = me.ws_heartbeat_interval.lock().await;

      select! {
        event = packets.recv() => {
          let event = event?;
          match TryInto::<GatewayEvent>::try_into(event) {
            Ok(event) => {
              debug!("<< {:?}", event);
            }

            Err(error) => {
              warn!("Failed to decode event: {}", error);
            }
          }
        },

        _ = async { interval.as_mut().unwrap().tick().await }, if interval.is_some() => {
          let mut ws_guard = me.ws.lock().await;
          let ws = ws_guard.as_mut().context("no voice gateway connection")?;

          ws.send_heartbeat().await?;
        }
      }
    }

    Ok(())
  }

  pub async fn run_udp_loop(me: Arc<Self>) -> Result<()> {
    let packet_size = TIMESTAMP_STEP * CHANNEL_COUNT;
    let buffer = HeapRb::<f32>::new(SAMPLE_RATE * 2); // TODO(Assasans): Calculate buffer size
    let (mut producer, mut consumer) = buffer.split();
    let (tx, rx) = flume::bounded(0);
    let (dtx, drx) = flume::bounded(0);

    let clone = me.clone();

    let ready = {
      let mut ws_lock = me.ws.lock().await;
      let ws = ws_lock.as_mut().context("no voice gateway connection")?;
      ws.ready.clone().context("no voice ready packet")?
    };

    tokio::task::spawn_blocking(move || {
      let mut data = vec![0f32; packet_size * 6];

      let mut sample_provider_lock = clone.sample_provider.blocking_lock();
      let sample_provider = sample_provider_lock.as_mut().context("no sample provider set").unwrap();

      'packet: loop {
        if producer.free_len() < data.len() {
          // warn!("sample buffer filled, blocking...");
          drx.recv().unwrap();
          continue;
        }

        let size = sample_provider.get_samples(&mut data);
        producer.push_slice(&data[..size]);
        if producer.len() >= packet_size {
          // debug!("wake sender");
          _ = tx.try_send(());
        }
        // got += size;

        // debug!("got {} samples", size);
        if size == 0 {
          break 'packet;
        }
      }
    });

    me.state.set(VoiceConnectionState::Playing).await?;
    'packet: loop {
      if consumer.len() < packet_size {
        warn!("sample buffer drained, waiting... {} / {}", consumer.len(), packet_size);
        rx.recv_async().await.unwrap();
      }

      while consumer.len() >= packet_size {
        let mut udp_lock = me.udp.lock().await;
        let udp = udp_lock.as_mut().context("no voice UDP socket")?;

        let mut data = vec![0f32; packet_size];
        consumer.pop_slice(&mut data);
        // debug!("sending {} samples", packet_size);

        if consumer.free_len() >= packet_size * 6 {
          // debug!("sample buffer drained");
          _ = dtx.try_send(());
        }

        me.send_voice_packet(&ready, udp, &data).await?;
        // samples.copy_within(packet_size..got, 0);
        // got -= packet_size;

        if Instant::now() >= udp.heartbeat_time + Duration::from_millis(5000) {
          udp.send_keepalive(&ready).await?;
        }
      }
    }

    me.state.set(VoiceConnectionState::Connected).await?;
    Ok(())
  }
}
