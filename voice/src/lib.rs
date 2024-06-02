pub mod buffer;
pub mod close_code;
pub mod constants;
pub mod event;
pub mod opcode;
pub mod provider;
pub mod udp;
pub mod ws;
mod rms;

use std::fmt::Debug;
use std::io;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use discortp::discord::{IpDiscoveryPacket, IpDiscoveryType, MutableIpDiscoveryPacket};
use discortp::rtcp::report::{MutableReceiverReportPacket, ReportBlockPacket};
use discortp::rtp::{MutableRtpPacket, RtpType};
use discortp::MutablePacket;
use flume::{Receiver, Sender};
pub use event::*;
pub use opcode::*;
use opus::{Application, Bitrate, Channels, Encoder};
use rand::random;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::select;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{interval, Interval};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tracing::*;
use utils::state_flow::StateFlow;
use xsalsa20poly1305::aead::generic_array::GenericArray;
use xsalsa20poly1305::{AeadInPlace, Key, KeyInit, XSalsa20Poly1305, TAG_SIZE};

use crate::buffer::SampleBuffer;
use crate::close_code::GatewayCloseCode;
use crate::constants::{
  CHANNEL_COUNT, CHUNK_DURATION, OPUS_SILENCE_FRAME, OPUS_SILENCE_FRAMES, SAMPLE_RATE, TIMESTAMP_STEP
};
use crate::provider::{SampleProvider, SampleProviderHandle};
use crate::rms::RMS;
use crate::udp::UdpVoiceConnection;
use crate::ws::{VoiceConnectionMode, WebSocketVoiceConnection};

#[derive(Debug, Serialize, Deserialize)]
pub struct GatewayPacket {
  #[serde(rename = "op")]
  opcode: GatewayOpcode,
  #[serde(rename = "d")]
  data: Option<Value>
}

impl GatewayPacket {
  pub fn new<T>(opcode: GatewayOpcode, data: T) -> Self
  where
    T: Into<Option<Value>>
  {
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

#[derive(Debug)]
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

#[derive(Debug)]
pub enum VoiceConnectionEvent {
  RmsPeak(f32)
}

pub struct VoiceConnection {
  pub ws: RwLock<Option<WebSocketVoiceConnection>>,
  ws_heartbeat_interval: Mutex<Option<Interval>>,
  pub udp: Mutex<Option<UdpVoiceConnection>>,
  cipher: Mutex<Option<XSalsa20Poly1305>>,
  cipher_mode: VoiceCipherMode,
  opus_encoder: Mutex<Encoder>,
  pub sample_provider: std::sync::Mutex<Option<Box<dyn SampleProvider>>>,
  pub sample_provider_handle: Mutex<Option<Box<dyn SampleProviderHandle>>>,
  pub state: StateFlow<VoiceConnectionState>,
  paused: StateFlow<bool>,
  silence_frames_left: AtomicU8,
  pub sample_buffer: SampleBuffer<f32>,
  pub rms: std::sync::Mutex<RMS<f32>>,
  pub stop_udp_loop: AtomicBool,
  events_tx: Sender<VoiceConnectionEvent>,
  pub events: Receiver<VoiceConnectionEvent>,
}

impl VoiceConnection {
  pub fn new() -> Result<Self> {
    let (events_tx, events_rx) = flume::bounded(16);

    Ok(Self {
      ws: RwLock::new(None),
      ws_heartbeat_interval: Mutex::new(None),
      udp: Mutex::new(None),
      cipher: Mutex::new(None),
      cipher_mode: VoiceCipherMode::Suffix,
      opus_encoder: Mutex::new(Encoder::new(48000, Channels::Stereo, Application::Audio)?),
      sample_provider: std::sync::Mutex::new(None),
      sample_provider_handle: Mutex::new(None),
      state: StateFlow::new(VoiceConnectionState::Disconnected),
      paused: StateFlow::new(false),
      silence_frames_left: AtomicU8::new(0),
      sample_buffer: SampleBuffer::new(SAMPLE_RATE * 3, SAMPLE_RATE, SAMPLE_RATE * 2),
      rms: std::sync::Mutex::new(RMS::new(((SAMPLE_RATE * CHANNEL_COUNT) as f32 * 0.025) as usize)),
      stop_udp_loop: AtomicBool::new(false),
      events_tx,
      events: events_rx
    })
  }

  pub async fn connect(&self, options: VoiceConnectionOptions) -> Result<()> {
    if let Some(bitrate) = options.bitrate {
      self
        .opus_encoder
        .lock()
        .await
        .set_bitrate(Bitrate::Bits(i32::try_from(bitrate)?))?;
    }
    debug!("using bitrate {:?}", self.opus_encoder.lock().await.get_bitrate());

    // self.opus_encoder.lock().await.set_inband_fec(true)?;
    // self.opus_encoder.lock().await.set_packet_loss_perc(50)?;

    debug!("connecting to gateway {}", options.endpoint);
    *self.ws.write().await = Some(WebSocketVoiceConnection::new(VoiceConnectionMode::New(options.clone())).await?);

    let ws = self.ws.read().await;
    let ws = ws.as_ref().context("no voice gateway connection")?;

    let hello = ws.hello.as_ref().context("no voice hello packet")?;
    let ready = ws.ready.as_ref().context("no voice ready packet")?;

    *self.ws_heartbeat_interval.lock().await =
      Some(interval(Duration::from_millis(hello.heartbeat_interval.round() as u64)));

    debug!("connecting to udp {}", options.endpoint);
    *self.udp.lock().await = Some(UdpVoiceConnection::new(ready).await?);

    let ip = self.discover_udp_ip(ready).await?;
    debug!("public ip: {:?}", ip);

    ws.send(
      GatewayEvent::SelectProtocol(SelectProtocol {
        protocol: "udp".to_owned(),
        data: SelectProtocolData {
          address: ip.address,
          port: ip.port,
          mode: "xsalsa20_poly1305_suffix".to_owned()
        }
      })
      .try_into()?
    )
    .await?;

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

    self.state.set(VoiceConnectionState::Connected);

    Ok(())
  }

  pub async fn disconnect(&self) -> Result<()> {
    self.state.set(VoiceConnectionState::Disconnected);
    *self.udp.lock().await = None;

    let mut ws_lock = self.ws.write().await;
    if let Some(ref ws) = *ws_lock {
      if !ws.is_closed() {
        ws.close(CloseFrame {
          code: CloseCode::Normal,
          reason: "".into()
        })
        .await?;
      }
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

    let null_index = view.get_address_raw().iter().position(|&b| b == 0).unwrap();

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
      AudioFrame::Pcm(data) => self.opus_encoder.lock().await.encode_float(
        &data,
        &mut payload[TAG_SIZE..TAG_SIZE + rtp_buffer_length - 12 - nonce_bytes.len()]
      )?
    };

    payload[TAG_SIZE + size..TAG_SIZE + size + nonce_bytes.len()].copy_from_slice(&nonce_bytes);

    let tag = cipher.encrypt_in_place_detached(nonce, b"", &mut payload[TAG_SIZE..TAG_SIZE + size]);
    match tag {
      Ok(tag) => {
        payload[..TAG_SIZE].copy_from_slice(tag.as_slice());

        spin_sleep::sleep(udp.deadline - Instant::now());
        let delta = Instant::now().saturating_duration_since(udp.deadline);
        udp.deadline = Instant::now() + CHUNK_DURATION;
        udp
          .socket
          .send(&udp.rtp_buffer[..12 + TAG_SIZE + size + nonce_bytes.len()])
          .await?;

        if delta > CHUNK_DURATION {
          warn!("Voice packet deadline exceeded by {:?}", delta - CHUNK_DURATION);
        }
      }
      Err(error) => {
        return Err(anyhow!(error));
      }
    }

    Ok(())
  }

  pub fn set_paused(&self, is_paused: bool) {
    self.paused.set(is_paused);
    self.rms.lock().unwrap().reset();
    if is_paused {
      self.silence_frames_left.store(OPUS_SILENCE_FRAMES, Ordering::Relaxed);
    } else {
      self.silence_frames_left.store(0, Ordering::Relaxed);
    }
  }

  pub fn is_paused(&self) -> bool {
    self.paused.get()
  }

  pub async fn run_ws_loop(me: Weak<Self>) -> Result<()> {
    let (read, close) = {
      let me = me.upgrade().context("voice connection dropped")?;
      let ws = me.ws.read().await;
      let ws = ws.as_ref().context("no voice gateway connection")?;

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

        _ = async { interval.as_mut().unwrap().tick().await }, if interval.is_some() => {
          let ws = me.ws.read().await;
          let ws = ws.as_ref().context("no voice gateway connection")?;

          match ws.send_heartbeat().await {
            Ok(_) => {},
            Err(error) => {
              debug!("websocket send heartbeat error: {:?}", error);
              break;
            }
          }
        }
      }
    }

    debug!("waiting for voice gateway closed event...");
    let frame = close.recv_async().await?;
    info!(?frame, "voice gateway closed");
    if let Some(frame) = frame {
      if let Some(me) = me.upgrade() {
        let code: GatewayCloseCode = frame.code.into();
        if code.can_reconnect() {
          me.reconnect_ws().await?;
        } else {
          debug!(?frame, "invalidating voice gateway connection");
          me.disconnect().await?;
        }
      } else {
        warn!("failed to upgrade weak me");
      }
    }

    Ok(())
  }

  pub async fn reconnect_ws(&self) -> Result<()> {
    let mut ws = self.ws.write().await;
    let old_ws = ws.take().expect("no voice gateway connection");

    debug!("reconnecting to voice gateway...");
    *ws = Some(
      WebSocketVoiceConnection::new(VoiceConnectionMode::Resume {
        options: old_ws.options,
        ready: old_ws.ready.context("no voice ready packet")?
      })
      .await?
    );
    Ok(())
  }

  pub async fn run_udp_loop(me: Arc<Self>) -> Result<()> {
    const PACKET_SIZE: usize = TIMESTAMP_STEP * CHANNEL_COUNT;
    let finished = Arc::new(AtomicBool::new(false));

    let clone = me.clone();
    let finished_clone = finished.clone();

    let ready = {
      let ws = me.ws.read().await;
      let ws = ws.as_ref().context("no voice gateway connection")?;
      ws.ready.clone().context("no voice ready packet")?
    };

    // TODO(Assasans): Seems like a hack...
    let (_udp_drop_tx, udp_drop_rx) = flume::bounded::<()>(0);
    tokio::task::spawn(async move {
      loop {
        let clone2 = clone.clone();
        let samples = tokio::task::spawn_blocking(move || {
          let mut sample_provider = clone2.sample_provider.lock().unwrap();
          let sample_provider = sample_provider.as_mut().context("no sample provider set").unwrap();
          sample_provider.get_samples()
        }).await.unwrap();

        match samples {
          Some(data) => {
            // debug!("got {} samples", data.len());
            select! {
              result = clone.sample_buffer.write(&data) => {
                result.unwrap();
              }

              _ = udp_drop_rx.recv_async() => {
                debug!("UDP loop exited, aborting IO task");
                break;
              }
            }
          }
          None => {
            debug!("got sample provider eof");
            break;
          }
        }
      }
      finished_clone.store(true, Ordering::Release);
    });

    debug!("waiting for jitter buffer to fill halfway");
    me.sample_buffer.wait_for(me.sample_buffer.low_threshold).await?;
    debug!("jitter buffer filled halfway");

    me.state.set(VoiceConnectionState::Playing);

    {
      let mut udp_lock = me.udp.lock().await;
      let udp = udp_lock.as_mut().context("no voice UDP socket")?;
      udp.deadline = Instant::now();
    }
    loop {
      if me.stop_udp_loop.load(Ordering::Relaxed) {
        debug!("stop udp loop");
        break;
      }

      let mut udp_lock = me.udp.lock().await;
      let udp = match udp_lock.as_mut() {
        Some(udp) => udp,
        None => {
          warn!("no voice UDP socket, possibly voice gateway was closed by remote");
          me.sample_buffer.clear().await;
          me.state.set(VoiceConnectionState::Disconnected);

          // Early return instead of break to prevent flushing to nonexistent connection
          return Ok(());
        }
      };

      if me.paused.get() && me.silence_frames_left.load(Ordering::Relaxed) > 0 {
        me.silence_frames_left.fetch_sub(1, Ordering::SeqCst);
        me.send_voice_packet(&ready, udp, AudioFrame::Opus(OPUS_SILENCE_FRAME.to_vec()))
          .await?;
        if me.silence_frames_left.load(Ordering::Relaxed) == 0 {
          debug!("waiting for unpause...");
          me.paused.wait_for(|paused| *paused == false).await;
          debug!("unpaused");
        }
      } else {
        // if let Ok(true) = me.jitter_buffer_reset.compare_exchange(true, false, Ordering::Acquire, Ordering::Relaxed) {
        //   debug!("reset sample buffer (was: {})", consumer.len());
        //   consumer.clear();
        //   me.jitter_buffer_size.store(0, Ordering::Relaxed);
        //   _ = dtx.try_send(()); // Unblock IO task
        //   continue;
        // }

        if finished.load(Ordering::Acquire) {
          debug!("got finished == true");
          break;
        }

        let mut data = vec![0f32; PACKET_SIZE];
        me.sample_buffer.read(&mut data).await?;
        // debug!("sending {} samples", PACKET_SIZE);

        let (rms, samples_len) = {
          let mut rms_lock = me.rms.lock().unwrap();
          for sample in &data {
            rms_lock.add_sample(*sample);
          }

          (rms_lock.calculate_rms(), rms_lock.samples.len())
        };

        if rms > 0.9 {
          info!("rms: {} over {} samples", rms, samples_len);
          // me.events_tx.send_async(VoiceConnectionEvent::RmsPeak(rms)).await.unwrap();
          me.send_voice_packet(&ready, udp, AudioFrame::Opus(OPUS_SILENCE_FRAME.to_vec())).await?;
          continue;
        }

        me.send_voice_packet(&ready, udp, AudioFrame::Pcm(data)).await?;
        // samples.copy_within(PACKET_SIZE..got, 0);
        // got -= PACKET_SIZE;
      }
      // me.recv_rtcp_stats(udp).await?;

      if Instant::now() >= udp.heartbeat_time + Duration::from_millis(5000) {
        udp.send_keepalive(&ready).await?;
      }
    }

    // Flush
    if !me.stop_udp_loop.load(Ordering::Relaxed) {
      let data = me.sample_buffer.flush().await;
      for chunk in data.chunks(PACKET_SIZE) {
        debug!("flushing {} (total: {}) samples...", chunk.len(), data.len());
        let mut chunk = chunk.to_vec();
        chunk.resize(PACKET_SIZE, 0f32); // Pad with zeros to make sure opus_encode_float does not fail

        let mut udp = me.udp.lock().await;
        let udp = udp.as_mut().context("no voice UDP socket")?;
        me.send_voice_packet(&ready, udp, AudioFrame::Pcm(chunk)).await?;
      }
    }

    debug!("play loop finished");
    me.sample_buffer.clear().await;
    me.state.set(VoiceConnectionState::Connected);
    Ok(())
  }
}
