mod opcode;
mod event;
mod provider;
mod ws;
mod udp;

use tokio::{sync::{RwLock, Mutex}, select, time::{interval, Interval}};
use tracing::*;
use std::{
  fmt::Debug,
  fs::File,
  net::IpAddr,
  str::FromStr,
  sync::Arc,
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
use serde::{Deserialize, Serialize};
use serde_json::Value;
use symphonia::core::{
  formats::FormatOptions,
  io::MediaSourceStream,
  meta::MetadataOptions,
  probe::Hint
};
use tokio_tungstenite::{tungstenite::protocol::{CloseFrame, frame::coding::CloseCode}};
use xsalsa20poly1305::{aead::generic_array::GenericArray, TAG_SIZE, XSalsa20Poly1305, KeyInit, Key, AeadInPlace};

pub use opcode::*;
pub use event::*;

use self::{provider::{SampleProvider, SymphoniaSampleProvider}, ws::WebSocketVoiceConnection, udp::UdpVoiceConnection};

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

pub struct VoiceConnection {
  ws: Mutex<Option<WebSocketVoiceConnection>>,
  ws_heartbeat_interval: Mutex<Option<Interval>>,
  udp: Mutex<Option<UdpVoiceConnection>>,
  cipher: Mutex<Option<XSalsa20Poly1305>>,
  cipher_mode: VoiceCipherMode,
  opus_encoder: Mutex<Encoder>,
  sample_provider: Mutex<Option<Box<dyn SampleProvider>>>
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
      sample_provider: Mutex::new(None)
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

    Ok(())
  }

  pub async fn disconnect(&self) -> Result<()> {
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

  pub async fn send_voice_packet(&self, ws: &WebSocketVoiceConnection, udp: &mut UdpVoiceConnection, input: &[f32]) -> Result<()> {
    let ready = ws.ready.as_ref().context("no voice ready packet")?;

    let cipher_guard = self.cipher.lock().await;
    let cipher = cipher_guard.as_ref().context("no voice cipher")?;

    let rtp_buffer_length = udp.rtp_buffer.len();
    let mut view = MutableRtpPacket::new(&mut *udp.rtp_buffer).unwrap();
    view.set_version(2);
    view.set_payload_type(RtpType::Unassigned(0x78));

    view.set_sequence(udp.sequence);
    udp.sequence += 1;

    view.set_timestamp(udp.timestamp);
    udp.timestamp += 48000 / 50;

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
    )?;
    payload[..TAG_SIZE].copy_from_slice(tag.as_slice());

    spin_sleep::sleep(udp.deadline - Instant::now());

    udp.socket.send(&udp.rtp_buffer[..12 + TAG_SIZE + size + nonce_bytes.len()]).await?;
    udp.deadline = Instant::now() + Duration::from_millis(1000 / 50 - 1);

    Ok(())
  }

  pub async fn run_ws_loop(me: Arc<Self>) -> Result<()> {
    let packets: async_channel::Receiver<GatewayPacket> = {
      let mut ws_guard = me.ws.lock().await;
      let ws = ws_guard.as_mut().context("no voice gateway connection")?;

      ws.packets.clone()
    };

    loop {
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
  }

  pub async fn run_udp_loop(me: Arc<Self>) -> Result<()> {
    let packet_size = 1920;
    let mut samples = [0f32; 48000]; // TODO(Assasans): Calculate buffer size
    let mut got = 0;

    let mut time = Instant::now();
    'packet: loop {
      let mut ws_lock = me.ws.lock().await;
      let ws = ws_lock.as_mut().context("no voice gateway connection")?;

      let mut udp_lock = me.udp.lock().await;
      let udp = udp_lock.as_mut().context("no voice UDP socket")?;

      let mut sample_provider_lock = me.sample_provider.lock().await;
      let sample_provider = sample_provider_lock.as_mut().context("no sample provider set")?;

      while got < packet_size {
        let size = sample_provider.get_samples(&mut samples[got..]);
        got += size;

        // debug!("got {} samples", size);
        if size == 0 {
          break 'packet;
        }
      }

      while got >= packet_size {
        debug!("sending {} samples", packet_size);
        me.send_voice_packet(ws, udp, &samples[..packet_size]).await?;
        samples.copy_within(packet_size..got, 0);
        got -= packet_size;

        let new_time = Instant::now();
        if new_time - time > Duration::from_millis(1000 / 50) {
          warn!("Voice packet deadline exceeded: {:?}", new_time - time);
        }
        time = new_time;
      }

      if Instant::now() >= udp.heartbeat_time + Duration::from_millis(5000) {
        let ready = ws.ready.as_ref().context("no voice ready packet")?;
        udp.send_keepalive(ready).await?;
      }
    }

    Ok(())
  }
}

pub async fn connect_voice_gateway(endpoint: &str, guild_id: u64, user_id: u64, session_id: &str, token: &str) -> Result<()> {
  let options = VoiceConnectionOptions {
    user_id,
    guild_id,
    bitrate: Some(96000),
    endpoint: endpoint.to_owned(),
    token: token.to_owned(),
    session_id: session_id.to_owned()
  };

  let connection = Arc::new(VoiceConnection::new()?);
  connection.connect(options).await?;
  {
    let mut ws_lock = connection.ws.lock().await;
    ws_lock.as_mut().unwrap().send_speaking(true).await?;
  }
  // {
  //   let connection = connection.clone();
  //   tokio::spawn(async move {
  //     VoiceConnection::run_loop(connection).await.unwrap();
  //   });
  // }

  // let file = File::open("/home/assasans/Downloads/output2.mp3")?;
  // let file = File::open("/home/assasans/Downloads/[Hi-Res] Chiisana Boukensha by Aqua, Megumin and Darkness/01 ちいさな冒険者 [ORT].flac")?;
  let file = File::open("/home/assasans/Downloads/Алла Пугачёва - Арлекино (minus 2).mp3")?;
  let source = MediaSourceStream::new(Box::new(file), Default::default());

  let mut hint = Hint::new();
  hint.with_extension("mp3");

  let probed = symphonia::default::get_probe()
    .format(&hint, source, &FormatOptions::default(), &MetadataOptions::default())
    .expect("unsupported format");

  *connection.sample_provider.lock().await = Some(Box::new(SymphoniaSampleProvider::new(probed)));

  let clone = connection.clone();
  tokio::spawn(async move {
    VoiceConnection::run_ws_loop(clone).await.unwrap()
  });

  let clone = connection.clone();
  tokio::spawn(async move {
    VoiceConnection::run_udp_loop(clone).await.unwrap();
  });

  Ok(())
}
