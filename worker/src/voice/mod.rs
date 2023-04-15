mod opcode;
mod event;
mod provider;

use discortp::wrap::{Wrap32, Wrap16};
use tokio::sync::{RwLock, Mutex};
use tracing::*;
use std::{
  fmt::{Debug, Formatter},
  fs::File,
  net::IpAddr,
  str::FromStr,
  sync::Arc,
  time::{Duration, SystemTime, Instant}
};
use opus::{Encoder, Bitrate, Channels, Application};
use anyhow::{Result, anyhow, Context};
use discortp::{
  MutablePacket,
  discord::{IpDiscoveryPacket, MutableIpDiscoveryPacket, IpDiscoveryType, MutableKeepalivePacket},
  rtp::{MutableRtpPacket, RtpType}
};
use futures_util::{SinkExt, StreamExt};
use rand::random;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use symphonia::core::{
  formats::FormatOptions,
  io::MediaSourceStream,
  meta::MetadataOptions,
  probe::Hint
};
use tokio::net::{TcpStream, UdpSocket};
use tokio_tungstenite::{tungstenite::protocol::Message, connect_async, WebSocketStream, MaybeTlsStream};
use xsalsa20poly1305::{aead::generic_array::GenericArray, TAG_SIZE, XSalsa20Poly1305, KeyInit, Key, AeadInPlace};

pub use opcode::*;
pub use event::*;

use self::provider::{SampleProvider, SymphoniaSampleProvider};

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

pub struct VoiceConnection {
  ws: Option<WebSocketStream<MaybeTlsStream<TcpStream>>>,
  udp: Option<UdpSocket>,
  cipher: Option<XSalsa20Poly1305>,
  cipher_mode: VoiceCipherMode,
  opus_encoder: Mutex<Encoder>,
  sample_provider: Option<Box<dyn SampleProvider>>,

  hello: Option<Hello>,
  ready: Option<Ready>,

  rtp_buffer: Vec<u8>,
  sequence: Wrap16,
  timestamp: Wrap32,
  deadline: Instant,
  ws_heartbeat_time: Instant,
  udp_keepalive_time: Instant
}

impl Debug for VoiceConnection {
  fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
    f.debug_struct("VoiceConnection")
      .field("ws", &self.ws)
      .field("udp", &self.udp)
      .field("cipher", &self.cipher.as_ref().map(|it| "XSalsa20Poly1305"))
      .field("cipher_mode", &self.cipher_mode)
      .field("opus_encoder", &self.opus_encoder)
      .finish()
  }
}

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

impl VoiceConnection {
  pub fn new() -> Result<Self> {
    Ok(Self {
      ws: None,
      udp: None,
      cipher: None,
      cipher_mode: VoiceCipherMode::Suffix,
      opus_encoder: Mutex::new(Encoder::new(48000, Channels::Stereo, Application::Audio)?),
      sample_provider: None,

      hello: None,
      ready: None,

      rtp_buffer: vec![0; 1460],
      sequence: random::<u16>().into(),
      timestamp: random::<u32>().into(),
      deadline: Instant::now(),
      ws_heartbeat_time: Instant::now(),
      udp_keepalive_time: Instant::now()
    })
  }

  pub async fn connect(&mut self, options: VoiceConnectionOptions) -> Result<()> {
    if let Some(bitrate) = options.bitrate {
      self.opus_encoder.lock().await.set_bitrate(Bitrate::Bits(i32::try_from(bitrate)?))?;
    }

    let (socket, _) = connect_async(format!("wss://{}/?v=4", options.endpoint)).await?;
    self.ws = Some(socket);
    debug!("voice gateway connected");

    self.send_gateway(GatewayEvent::Identify(Identify {
      server_id: options.guild_id,
      user_id: options.user_id,
      session_id: options.session_id,
      token: options.token
    }).try_into()?).await?;

    let mut hello = None;
    let mut ready = None;
    loop {
      let event: GatewayEvent = self.receive_gateway().await?.try_into()?;
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

    self.hello = Some(hello.unwrap());
    self.ready = Some(ready.unwrap());

    self.ws_heartbeat_time = Instant::now(); // Reset time

    self.connect_udp().await?;

    Ok(())
  }

  async fn connect_udp(&mut self) -> Result<()> {
    let ready = self.ready.as_ref().context("no voice ready packet")?;

    let udp = self.udp.insert(UdpSocket::bind("0.0.0.0:0").await?);
    udp.connect((ready.ip.clone(), ready.port)).await?;

    self.udp_keepalive_time = Instant::now(); // Reset time

    let ip = self.discover_udp_ip().await?;

    self.send_gateway(GatewayEvent::SelectProtocol(SelectProtocol {
      protocol: "udp".to_owned(),
      data: SelectProtocolData {
        address: ip.address,
        port: ip.port,
        mode: "xsalsa20_poly1305_suffix".to_owned()
      }
    }).try_into()?).await?;

    let session_description = loop {
      // Ignore undocumented opcode 18
      let event: GatewayEvent = match self.receive_gateway().await?.try_into() {
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
    self.cipher = Some(XSalsa20Poly1305::new(&key));

    Ok(())
  }

  async fn send_udp_keepalive(&mut self) -> Result<()> {
    let udp = self.udp.as_mut().context("no voice UDP socket")?;
    let ready = self.ready.as_ref().context("no voice ready packet")?;

    let mut buffer = [0; MutableKeepalivePacket::minimum_packet_size()];
    let mut view = MutableKeepalivePacket::new(&mut buffer[..]).unwrap();
    view.set_ssrc(ready.ssrc);

    udp.send(&buffer[..]).await?;
    debug!("Sent UDP keepalive");

    self.udp_keepalive_time = Instant::now();

    Ok(())
  }

  async fn send_gateway_heartbeat(&mut self) -> Result<()> {
    let nonce = u64::try_from(SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?.as_millis())?;
    self.send_gateway(GatewayEvent::Heartbeat(nonce).try_into()?).await?;

    let ack_nonce = {
      let event: GatewayEvent = self.receive_gateway().await?.try_into()?;
      match event {
        GatewayEvent::HeartbeatAck(nonce) => nonce,
        other => {
          warn!("Expected HeartbeatAck packet, got: {:?}", other);
          return Err(anyhow!("Invalid packet")); // TODO
        }
      }
    };

    debug!("Sent gateway heartbeat, ack nonce: {}", ack_nonce);

    self.ws_heartbeat_time = Instant::now();

    Ok(())
  }

  async fn discover_udp_ip(&mut self) -> Result<IpDiscoveryResult> {
    let udp = self.udp.as_mut().context("no voice UDP socket")?;
    let ready = self.ready.as_ref().context("no voice ready packet")?;

    let mut buffer = [0; IpDiscoveryPacket::const_packet_size()];
    let mut view = MutableIpDiscoveryPacket::new(&mut buffer[..]).unwrap();
    view.set_pkt_type(IpDiscoveryType::Request);
    view.set_length(70);
    view.set_ssrc(ready.ssrc);
    udp.send(&buffer).await?;

    let (length, _address) = udp.recv_from(&mut buffer).await?;
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

  pub async fn send_voice_packet(&mut self, input: &[f32]) -> Result<()> {
    let udp = self.udp.as_mut().context("no voice UDP socket")?;
    let ready = self.ready.as_ref().context("no voice ready packet")?;
    let cipher = self.cipher.as_mut().context("no voice cipher")?;

    let rtp_buffer_length = self.rtp_buffer.len();
    let mut view = MutableRtpPacket::new(&mut self.rtp_buffer).unwrap();
    view.set_version(2);
    view.set_payload_type(RtpType::Unassigned(0x78));
    view.set_sequence(self.sequence);
    self.sequence += 1;
    view.set_timestamp(self.timestamp);
    self.timestamp += 48000 / 50;
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

    spin_sleep::sleep(self.deadline - Instant::now());

    udp.send(&self.rtp_buffer[..12 + TAG_SIZE + size + nonce_bytes.len()]).await?;
    self.deadline = Instant::now() + Duration::from_millis(1000 / 50 - 1);

    Ok(())
  }

  pub async fn send_speaking(&mut self, speaking: bool) -> Result<()> {
    let ready = self.ready.as_ref().context("no voice ready packet")?;

    self.send_gateway(GatewayEvent::Speaking(Speaking {
      speaking: if speaking { 1 } else { 0 },
      delay: 0,
      ssrc: ready.ssrc
    }).try_into()?).await?;

    Ok(())
  }

  pub async fn send_gateway(&mut self, packet: GatewayPacket) -> Result<()> {
    let ws = self.ws.as_mut().context("no voice gateway connection")?;

    let json = serde_json::to_string(&packet)?;
    ws.send(Message::Text(json)).await?;
    ws.flush().await?;

    Ok(())
  }

  pub async fn receive_gateway(&mut self) -> Result<GatewayPacket> {
    let ws = self.ws.as_mut().context("no voice gateway connection")?;

    let message = ws.next().await.context("no next voice gateway message")??;
    let json = message.into_text()?;
    debug!("< {}", json);

    Ok(serde_json::from_str(&json)?)
  }

  pub async fn run_loop(me: Arc<RwLock<Self>>) -> Result<()> {
    let packet_size = 1920;
    let mut samples = [0f32; 48000];
    let mut got = 0;

    let mut time = Instant::now();
    loop {
      let mut lock = me.write().await;
      let sample_provider = lock.sample_provider.as_mut().context("no sample provider set")?;

      while got < packet_size {
        let size = sample_provider.get_samples(&mut samples[got..]);
        got += size;

        // debug!("got {} samples", size);
        if size == 0 {
          break;
        }
      }

      if got == 0 {
        continue;
      }

      // debug!("sending {} samples", packet_size);
      lock.send_voice_packet(&samples[..packet_size]).await?;
      samples.copy_within(packet_size..got, 0);
      got -= packet_size;

      let new_time = Instant::now();
      if new_time - time > Duration::from_millis(1000 / 50) {
        warn!("Voice packet deadline exceeded: {:?}", new_time - time);
      }
      time = new_time;

      if Instant::now() >= lock.ws_heartbeat_time + Duration::from_millis(lock.hello.as_ref().unwrap().heartbeat_interval.round() as u64) {
        lock.send_gateway_heartbeat().await?;
      }

      if Instant::now() >= lock.udp_keepalive_time + Duration::from_millis(5000) {
        lock.send_udp_keepalive().await?;
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

  let connection = Arc::new(RwLock::new(VoiceConnection::new()?));
  {
    let mut connection = connection.write().await;
    connection.connect(options).await?;
    connection.send_speaking(true).await?;
  }
  // {
  //   let connection = connection.clone();
  //   tokio::spawn(async move {
  //     VoiceConnection::run_loop(connection).await.unwrap();
  //   });
  // }

  // let file = File::open("/home/assasans/Downloads/output2.mp3")?;
  let file = File::open("/home/assasans/Downloads/[Hi-Res] Chiisana Boukensha by Aqua, Megumin and Darkness/01 ちいさな冒険者 [ORT].flac")?;
  // let file = File::open("/home/assasans/Downloads/Алла Пугачёва - Арлекино (minus 2).mp3")?;
  let source = MediaSourceStream::new(Box::new(file), Default::default());

  let mut hint = Hint::new();
  hint.with_extension("mp3");

  let probed = symphonia::default::get_probe()
    .format(&hint, source, &FormatOptions::default(), &MetadataOptions::default())
    .expect("unsupported format");

  connection.write().await.sample_provider = Some(Box::new(SymphoniaSampleProvider::new(probed)));
  VoiceConnection::run_loop(connection).await.unwrap();

  Ok(())
}
