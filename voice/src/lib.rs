pub mod constants;
pub mod opcode;
pub mod event;
pub mod provider;
pub mod ws;
pub mod udp;
pub mod close_code;

use tokio::{sync::Mutex, select, time::{interval, Interval}};
use tracing::*;
use std::{
  io,
  fmt::Debug,
  net::IpAddr,
  str::FromStr,
  sync::{Arc, Weak},
  time::{Duration, Instant}
};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use opus::{Encoder, Bitrate, Channels, Application};
use anyhow::{Result, anyhow, Context};
use discortp::{
  MutablePacket,
  discord::{IpDiscoveryPacket, MutableIpDiscoveryPacket, IpDiscoveryType},
  rtp::{MutableRtpPacket, RtpType},
  rtcp::report::{MutableReceiverReportPacket, ReportBlockPacket}
};
use flume::RecvError;
use rand::random;
use ringbuf::HeapRb;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_tungstenite::{tungstenite::protocol::{CloseFrame, frame::coding::CloseCode}};
use xsalsa20poly1305::{aead::generic_array::GenericArray, TAG_SIZE, XSalsa20Poly1305, KeyInit, Key, AeadInPlace};

pub use opcode::*;
pub use event::*;

use utils::state_flow::StateFlow;
use crate::close_code::GatewayCloseCode;
use crate::constants::{OPUS_SILENCE_FRAME, OPUS_SILENCE_FRAMES};
use crate::provider::SampleProviderHandle;
use crate::ws::VoiceConnectionMode;
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

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum VoiceConnectionState {
  Disconnected,
  Connected,
  Playing
}

#[derive(Debug, PartialEq)]
pub enum AudioFrame {
  Opus(Vec<u8>),
  Pcm(Vec<f32>)
}

pub struct VoiceConnection {
  pub ws: Mutex<Option<WebSocketVoiceConnection>>,
  ws_heartbeat_interval: Mutex<Option<Interval>>,
  pub udp: Mutex<Option<UdpVoiceConnection>>,
  cipher: Mutex<Option<XSalsa20Poly1305>>,
  cipher_mode: VoiceCipherMode,
  opus_encoder: Mutex<Encoder>,
  pub sample_provider: Mutex<Option<Box<dyn SampleProvider>>>,
  pub sample_provider_handle: Mutex<Option<Box<dyn SampleProviderHandle>>>,
  pub state: StateFlow<VoiceConnectionState>,
  paused: StateFlow<bool>,
  silence_frames_left: AtomicU8,
  pub jitter_buffer_size: AtomicUsize,
  pub jitter_buffer_reset: AtomicBool,
  pub stop_udp_loop: AtomicBool
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
      sample_provider_handle: Mutex::new(None),
      state: StateFlow::new(VoiceConnectionState::Disconnected),
      paused: StateFlow::new(false),
      silence_frames_left: AtomicU8::new(0),
      jitter_buffer_size: AtomicUsize::new(0),
      jitter_buffer_reset: AtomicBool::new(false),
      stop_udp_loop: AtomicBool::new(false)
    })
  }

  pub async fn connect(&self, options: VoiceConnectionOptions) -> Result<()> {
    if let Some(bitrate) = options.bitrate {
      self.opus_encoder.lock().await.set_bitrate(Bitrate::Bits(i32::try_from(bitrate)?))?;
    }
    // self.opus_encoder.lock().await.set_inband_fec(true)?;
    // self.opus_encoder.lock().await.set_packet_loss_perc(50)?;

    *self.ws.lock().await = Some(WebSocketVoiceConnection::new(VoiceConnectionMode::New(options.clone())).await?);

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

    self.state.set(VoiceConnectionState::Connected)?;

    Ok(())
  }

  pub async fn disconnect(&self) -> Result<()> {
    self.state.set(VoiceConnectionState::Disconnected)?;
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

  pub fn is_connected(&self) -> bool {
    self.state.get() != VoiceConnectionState::Disconnected
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

  pub async fn recv_rtcp_stats(&self, udp: &mut UdpVoiceConnection) -> Result<()> {
    let mut buffer = [0; 4096];
    let (length, _address) = match udp.socket.try_recv_from(&mut buffer) {
      Ok((length, address)) => (length, address),
      Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
      Err(error) => return Err(anyhow::anyhow!(error))
    };

    let mut nonce_bytes = [0; 24];
    nonce_bytes.copy_from_slice(&buffer[length - 24..length]);
    let nonce = GenericArray::from_slice(&nonce_bytes);

    let mut view = MutableReceiverReportPacket::new(&mut buffer[..length - 24]).unwrap();

    let mut tag_bytes = [0; TAG_SIZE];
    tag_bytes.copy_from_slice(&view.payload_mut()[..TAG_SIZE]);
    let tag = GenericArray::from_slice(&tag_bytes);

    let cipher_guard = self.cipher.lock().await;
    let cipher = cipher_guard.as_ref().context("no voice cipher")?;

    let data = &mut view.payload_mut()[TAG_SIZE..];

    cipher.decrypt_in_place_detached(nonce, b"", data, tag).unwrap();

    // TODO(Assasans): Support view.rx_report_count != 1
    let report = ReportBlockPacket::new(data).unwrap();
    debug!("{report:?}");

    Ok(())
  }

  pub async fn send_voice_packet(&self, ready: &Ready, udp: &mut UdpVoiceConnection, frame: AudioFrame) -> Result<()> {
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

    let size = match frame {
      AudioFrame::Opus(data) => {
        payload[TAG_SIZE..TAG_SIZE + data.len()].copy_from_slice(&data);
        data.len()
      }
      AudioFrame::Pcm(data) => {
        self.opus_encoder.lock().await.encode_float(&data, &mut payload[TAG_SIZE..TAG_SIZE + rtp_buffer_length - 12 - nonce_bytes.len()])?
      }
    };

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
        udp.socket.send(&udp.rtp_buffer[..12 + TAG_SIZE + size + nonce_bytes.len()]).await?;

        if delta > CHUNK_DURATION {
          warn!("Voice packet deadline exceeded by {:?}", delta - CHUNK_DURATION);
        }
      },
      Err(error) => {
        return Err(anyhow!(error));
      }
    }

    Ok(())
  }

  pub fn set_paused(&self, is_paused: bool) -> Result<()> {
    self.paused.set(is_paused)?;
    if is_paused {
      self.silence_frames_left.store(OPUS_SILENCE_FRAMES, Ordering::Relaxed);
    } else {
      self.silence_frames_left.store(0, Ordering::Relaxed);
    }
    Ok(())
  }

  pub fn is_paused(&self) -> bool {
    self.paused.get()
  }

  pub async fn run_ws_loop(me: Weak<Self>) -> Result<()> {
    let (read, close) = {
      let me = me.upgrade().context("voice connection dropped")?;
      let mut ws = me.ws.lock().await;
      let ws = ws.as_mut().context("no voice gateway connection")?;

      (ws.read.clone(), ws.close_rx.clone())
    };

    while let Some(me) = me.upgrade() {
      let mut interval = me.ws_heartbeat_interval.lock().await;

      select! {
        event = read.recv_async() => {
          let event = match event {
            Ok(event) => event,
            Err(error) => {
              debug!("websocket read error: {:?}", error);
              break;
            }
          };

          match TryInto::<GatewayEvent>::try_into(event) {
            Ok(event) => {
              debug!("<< {:?}", event);
            }

            Err(error) => {
              warn!("Failed to decode event: {}", error);
            }
          }
        }

        frame = close.recv_async() => {
          let frame = match frame {
            Ok(frame) => frame,
            Err(error) => {
              debug!("websocket close channel error: {:?}", error);
              break;
            }
          };

          info!(?frame, "voice gateway closed");
          if let Some(frame) = frame {
            let code: GatewayCloseCode = frame.code.into();
            if code.can_reconnect() {
              let mut ws = me.ws.lock().await;
              let old_ws = ws.take().context("no voice gateway connection")?;

              *ws = Some(WebSocketVoiceConnection::new(VoiceConnectionMode::Resume {
                options: old_ws.options,
                hello: old_ws.hello.context("no voice hello packet")?,
                ready: old_ws.ready.context("no voice ready packet")?
              }).await?);
            }
          }
        }

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
    let buffer = HeapRb::<f32>::new(SAMPLE_RATE * 3); // TODO(Assasans): Calculate buffer size
    let (mut producer, mut consumer) = buffer.split();
    let (stx, srx) = flume::bounded(0);
    let (tx, rx) = flume::bounded(0);
    let (dtx, drx) = flume::bounded(0);

    let clone = me.clone();

    let ready = {
      let mut ws_lock = me.ws.lock().await;
      let ws = ws_lock.as_mut().context("no voice gateway connection")?;
      ws.ready.clone().context("no voice ready packet")?
    };

    tokio::task::spawn_blocking(move || {
      let mut sample_provider_lock = clone.sample_provider.blocking_lock();

      'packet: loop {
        let sample_provider = sample_provider_lock.as_mut().context("no sample provider set").unwrap();

        if producer.len() >= producer.capacity() / 2 {
          // Ready to play, filled
          _ = stx.try_send(());
        }

        match sample_provider.get_samples() {
          Some(data) => {
            if producer.free_len() < data.len() {
              warn!("jitter buffer filled ({} < {}), blocking sample provider loop...", producer.free_len(), data.len());
              match drx.recv() {
                Ok(()) => {},
                Err(error) if error == RecvError::Disconnected => break,
                Err(error) => panic!("drx.recv(): {:?}", error)
              };
            }

            producer.push_slice(&data);
            clone.jitter_buffer_size.fetch_add(data.len(), Ordering::Relaxed);
            if producer.len() >= packet_size {
              // debug!("wake sender");
              _ = tx.try_send(());
            }

            // debug!("got {} samples", data.len());
          }
          None => {
            debug!("got sample provider eof");
            break 'packet;
          }
        }
      }
    });

    debug!("waiting for jitter buffer to fill halfway");
    srx.recv_async().await?;
    debug!("jitter buffer filled halfway");

    me.state.set(VoiceConnectionState::Playing)?;

    {
      let mut udp_lock = me.udp.lock().await;
      let udp = udp_lock.as_mut().context("no voice UDP socket")?;
      udp.deadline = Instant::now();
    }
    'packet: loop {
      if consumer.len() < packet_size {
        warn!("sample buffer drained, waiting... {} / {}", consumer.len(), packet_size);
        match rx.recv_async().await {
          Ok(()) => {},
          Err(error) if error == RecvError::Disconnected => break,
          Err(error) => return Err(anyhow::anyhow!(error))
        };
      }

      while consumer.len() >= packet_size {
        if me.stop_udp_loop.load(Ordering::Relaxed) {
          debug!("stop udp loop");
          break 'packet;
        }

        let mut udp_lock = me.udp.lock().await;
        let udp = udp_lock.as_mut().context("no voice UDP socket")?;

        if me.paused.get() && me.silence_frames_left.load(Ordering::Relaxed) > 0 {
          me.silence_frames_left.fetch_sub(1, Ordering::SeqCst);
          me.send_voice_packet(&ready, udp, AudioFrame::Opus(OPUS_SILENCE_FRAME.to_vec())).await?;
          if me.silence_frames_left.load(Ordering::Relaxed) == 0 {
            debug!("waiting for unpause...");
            me.paused.wait_for(|paused| *paused == false).await?;
            debug!("unpaused");
          }
        } else {
          if let Ok(true) = me.jitter_buffer_reset.compare_exchange(true, false, Ordering::Acquire, Ordering::Relaxed) {
            debug!("reset sample buffer (was: {})", consumer.len());
            consumer.clear();
            me.jitter_buffer_size.store(0, Ordering::Relaxed);
            _ = dtx.try_send(()); // Unblock IO task
            continue;
          }

          let mut data = vec![0f32; packet_size];
          consumer.pop_slice(&mut data);
          me.jitter_buffer_size.fetch_sub(data.len(), Ordering::Relaxed);
          // debug!("sending {} samples", packet_size);

          if consumer.free_len() >= consumer.capacity() / 2 {
            // debug!("sample buffer drained");
            _ = dtx.try_send(());
          }

          me.send_voice_packet(&ready, udp, AudioFrame::Pcm(data)).await?;
          // samples.copy_within(packet_size..got, 0);
          // got -= packet_size;
        }
        // me.recv_rtcp_stats(udp).await?;

        if Instant::now() >= udp.heartbeat_time + Duration::from_millis(5000) {
          udp.send_keepalive(&ready).await?;
        }
      }
    }

    if !me.stop_udp_loop.load(Ordering::Relaxed) {
      // Flush
      let len = consumer.len();
      let mut data = vec![0f32; packet_size];
      debug!("flushing {} samples...", len);
      consumer.pop_slice(&mut data[..len]);

      let mut udp_lock = me.udp.lock().await;
      let udp = udp_lock.as_mut().context("no voice UDP socket")?;
      me.send_voice_packet(&ready, udp, AudioFrame::Pcm(data)).await?;
    }

    debug!("play loop finished");
    me.state.set(VoiceConnectionState::Connected)?;
    Ok(())
  }
}
