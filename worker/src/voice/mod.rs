use std::fs::File;
use std::io::{Error, ErrorKind, Read, Write, Cursor};
use std::net::{TcpStream, IpAddr};
use std::str::FromStr;
use std::time::Duration;
use std::{thread, time};
use audiopus::coder::{Encoder, Decoder};
use audiopus::packet::Packet;
use audiopus::{Bitrate, Channels, Application, SampleRate, MutSignals};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use anyhow::{Result, anyhow};
use discortp::MutablePacket;
use discortp::discord::{IpDiscoveryPacket, MutableIpDiscoveryPacket, IpDiscoveryType, MutableKeepalivePacket};
use discortp::rtp::{MutableRtpPacket, RtpPacket, RtpType};
use futures_util::{SinkExt, StreamExt};
use opus_mux::Demuxer;
use rand::random;
use serde::{Deserialize, Serialize};
use serde_json::json;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::get_codecs;
use tokio::net::{TcpStream as TokioTcpStream, UdpSocket};
use tokio::time::sleep;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::WebSocketStream;
use xsalsa20poly1305::aead::generic_array::GenericArray;
use xsalsa20poly1305::aead::{Aead, Nonce, Payload};
use xsalsa20poly1305::{TAG_SIZE, XSalsa20Poly1305, KeyInit, Key, AeadInPlace};


#[derive(Debug, Serialize, Deserialize)]
struct VoiceGatewayPacket {
  #[serde(rename = "op")]
  opcode: u8,
  #[serde(rename = "d")]
  data: Option<serde_json::Value>
}

#[derive(Debug, Serialize, Deserialize)]
struct ReadyData {
  ssrc: u32,
  ip: String,
  port: u16,
  modes: Vec<String>
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionDescriptionData {
  mode: String,
  secret_key: Vec<u8>
}

#[derive(Debug)]
struct VoicePacket {
  version_flags: u8,
  payload_type: u8,
  sequence: u16,
  timestamp: u32,
  ssrc: u32,
  encrypted_audio: Vec<u8>,
}

impl VoicePacket {
  pub fn new(sequence: u16, timestamp: u32, ssrc: u32, encrypted_audio: Vec<u8>) -> Self {
    Self {
      version_flags: 0x80,
      payload_type: 0x78,
      sequence,
      timestamp,
      ssrc,
      encrypted_audio,
    }
  }

  pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
    let mut reader = std::io::Cursor::new(bytes);
    let version_flags = reader.read_u8()?;
    let payload_type = reader.read_u8()?;
    let sequence = reader.read_u16::<BigEndian>()?;
    let timestamp = reader.read_u32::<BigEndian>()?;
    let ssrc = reader.read_u32::<BigEndian>()?;
    let mut encrypted_audio = vec![0; bytes.len() - 12];
    reader.read_exact(&mut encrypted_audio)?;

    Ok(Self {
      version_flags,
      payload_type,
      sequence,
      timestamp,
      ssrc,
      encrypted_audio,
    })
  }

  pub fn to_bytes(&self) -> Result<Vec<u8>> {
    let mut writer = Vec::new();
    writer.write_u8(self.version_flags)?;
    writer.write_u8(self.payload_type)?;
    writer.write_u16::<BigEndian>(self.sequence)?;
    writer.write_u32::<BigEndian>(self.timestamp)?;
    writer.write_u32::<BigEndian>(self.ssrc)?;
    writer.write_all(&self.encrypted_audio)?;

    Ok(writer)
  }
}

pub async fn connect_voice_gateway(endpoint: &str, guild_id: u64, user_id: u64, session_id: &str, token: &str) -> Result<()> {
  let (mut socket, _) = connect_async(format!("wss://{}/?v=4", endpoint)).await?;
  println!("voice gateway connected");

  let identify_payload = VoiceGatewayPacket {
    opcode: 0,
    data: Some(json!({
      "server_id": guild_id,
      "user_id": user_id,
      "session_id": session_id,
      "token": token
    }))
  };

  let identify_message = Message::Text(serde_json::to_string(&identify_payload).unwrap());
  socket.send(identify_message).await?;

  println!("waiting for hello...");
  let hello = {
    let message = socket.next().await.unwrap()?;
    let packet: VoiceGatewayPacket = serde_json::from_str(&message.to_string())?;

    if packet.opcode != 8 {
      return Err(anyhow!(format!("Invalid opcode {}", packet.opcode)));
    }
  };

  println!("waiting for ready...");
  let ready: ReadyData = {
    let message = socket.next().await.unwrap()?;
    let packet: VoiceGatewayPacket = serde_json::from_str(&message.to_string())?;

    if packet.opcode != 2 {
      return Err(anyhow!(format!("Invalid opcode {}", packet.opcode)));
    }

    serde_json::from_value(packet.data.unwrap())?
  };
  println!("Ready: {:?}", ready);

  let udp = UdpSocket::bind("0.0.0.0:0").await?;
  udp.connect((ready.ip, ready.port)).await?;

  // Follow Discord's IP Discovery procedures, in case NAT tunnelling is needed.
  let mut bytes = [0; IpDiscoveryPacket::const_packet_size()];
  {
    let mut view = MutableIpDiscoveryPacket::new(&mut bytes[..]).unwrap();
    view.set_pkt_type(IpDiscoveryType::Request);
    view.set_length(70);
    view.set_ssrc(ready.ssrc);
  }
  udp.send(&bytes).await?;

  let (len, _addr) = udp.recv_from(&mut bytes).await?;
  {
    let view = IpDiscoveryPacket::new(&bytes[..len]).unwrap();
    if view.get_pkt_type() != IpDiscoveryType::Response {
      return Err(anyhow!("Invalid response"));
    }

    let nul_byte_index = view
      .get_address_raw()
      .iter()
      .position(|&b| b == 0)
      .unwrap();

    let address = std::str::from_utf8(&view.get_address_raw()[..nul_byte_index])
      .map(|it| IpAddr::from_str(it))??;
    println!("Address: {:?}", address);

    let select_message = Message::Text(serde_json::to_string(&json!({
      "op": 1,
      "d": {
        "protocol": "udp",
        "data": {
          "address": address,
          "port": view.get_port(),
          "mode": "xsalsa20_poly1305_suffix"
        }
      }
    })).unwrap());
    socket.send(select_message).await?;
  }

  let select_message = Message::Text(serde_json::to_string(&json!({
    "op": 5,
    "d": {
      "speaking": 1,
      "delay": 0,
      "ssrc": ready.ssrc
    }
  })).unwrap());
  socket.send(select_message).await?;
  socket.flush().await?;

  let session_description: SessionDescriptionData = loop {
    let message = socket.next().await.unwrap()?;
    let packet: VoiceGatewayPacket = serde_json::from_str(&message.to_string())?;

    if packet.opcode != 4 {
      println!("Invalid opcode {}", packet.opcode);
      continue;
    }

    break serde_json::from_value(packet.data.unwrap())?
  };
  println!("Session desciption: {:?}", session_description);

  let mut file = File::open("/home/assasans/Downloads/output.opus")?;
  // let source = MediaSourceStream::new(Box::new(file), Default::default());

  // let mut hint = Hint::new();
  // hint.with_extension("mp3");

  // // Use the default options for metadata and format readers.
  // let meta_opts: MetadataOptions = Default::default();
  // let fmt_opts: FormatOptions = Default::default();

  // // Probe the media source.
  // let probed = symphonia::default::get_probe()
  //   .format(&hint, source, &fmt_opts, &meta_opts)
  //   .expect("unsupported format");

  // // Get the instantiated format reader.
  // let mut format = probed.format;

  // // Find the first audio track with a known (decodeable) codec.
  // let track = format
  //   .tracks()
  //   .iter()
  //   .find(|it| it.codec_params.codec != CODEC_TYPE_NULL)
  //   .expect("no supported audio tracks");

  // // Use the default options for the decoder.
  // let dec_opts: DecoderOptions = Default::default();

  // // Create a decoder for the track.
  // let mut decoder = get_codecs()
  //   .make(&track.codec_params, &dec_opts)
  //   .expect("unsupported codec");

  // // Store the track identifier, it will be used to filter packets.
  // let track_id = track.id;

  let mut sequence = random::<u16>();
  let mut timestamp = random::<u32>();
  let mut nonce = 0;

  let mut encoder = Encoder::new(SampleRate::Hz48000, Channels::Stereo, Application::Audio)?;
  encoder.set_bitrate(Bitrate::BitsPerSecond(96000))?;

  let mut opus_decoder = Decoder::new(SampleRate::Hz48000, Channels::Stereo)?;

  let mut rtp_packet = [0; 12 + 15 + 24 + 16]; // 1460];

  // let mut sample_count = 0;
  // let mut sample_buf = None;

  let key = Key::from_slice(&session_description.secret_key);
  let cipher = XSalsa20Poly1305::new(&key);
  let dcipher = XSalsa20Poly1305::new(&key);

  {
    let mut buf = [0; MutableKeepalivePacket::minimum_packet_size()];
    let mut view = MutableKeepalivePacket::new(&mut rtp_packet[..]).unwrap();
    view.set_ssrc(ready.ssrc);

    udp.send(&buf[..]).await?;
  }

  let mut demuxer = Demuxer::new();
  loop {
    let mut buf = [0; 8192];
    let size = file.read(&mut buf)?;
    if size == 0 {
      break;
    }

    demuxer.push(&buf)?;
  }

  loop {
    // Get the next packet from the media format.
    // let packet = match format.next_packet() {
    let mut buf = vec![0xf8, 0xff, 0xfe, 0xf8, 0xff, 0xfe, 0xf8, 0xff, 0xfe, 0xf8, 0xff, 0xfe, 0xf8, 0xff, 0xfe]; // demuxer.next().unwrap();
    //   Ok(packet) => packet,
    //   Err(symphonia::core::errors::Error::ResetRequired) => {
    //     // The track list has been changed. Re-examine it and create a new set of decoders,
    //     // then restart the decode loop. This is an advanced feature and it is not
    //     // unreasonable to consider this "the end." As of v0.5.0, the only usage of this is
    //     // for chained OGG physical streams.
    //     unimplemented!();
    //   }
    //   Err(err) => {
    //     // A unrecoverable error occured, halt decoding.
    //     panic!("{}", err);
    //   }
    // };

    // // Consume any new metadata that has been read since the last packet.
    // while !format.metadata().is_latest() {
    //   // Pop the old head of the metadata queue.
    //   format.metadata().pop();

    //   // Consume the new metadata at the head of the metadata queue.
    // }

    // // If the packet does not belong to the selected track, skip over it.
    // if packet.track_id() != track_id {
    //   continue;
    // }

    // // Decode the packet into audio samples.
    // match decoder.decode(&packet) {
    //   Ok(buffer) => {
    //     // If this is the *first* decoded packet, create a sample buffer matching the
    //     // decoded audio buffer format.
    //     if sample_buf.is_none() {
    //       // Get the audio buffer specification.
    //       let spec = *buffer.spec();

    //       // Get the capacity of the decoded buffer. Note: This is capacity, not length!
    //       let duration = buffer.capacity() as u64;

    //       // Create the f32 sample buffer.
    //       sample_buf = Some(SampleBuffer::<i16>::new(duration, spec));
    //     }

    //     // Copy the decoded audio buffer into the sample buffer in an interleaved format.
    //     if let Some(buf) = &mut sample_buf {
    //       buf.copy_interleaved_ref(buffer);

    //       // The samples may now be access via the `samples()` function.
    //       sample_count += buf.samples().len();
    //       println!("Decoded {} samples", sample_count);

          let mut view = MutableRtpPacket::new(&mut rtp_packet[..]).unwrap();
          view.set_version(2);
          view.set_payload_type(RtpType::Unassigned(0x78));
          view.set_sequence(sequence.into());
          sequence += 1;
          view.set_timestamp(timestamp.into());
          timestamp += 100;
          view.set_ssrc(ready.ssrc);

          let payload = view.payload_mut();
          let mut plaintext = buf; // Vec::new();
          // let mut plaintext = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
          let size = std::cmp::min(plaintext.len(), 1460 - TAG_SIZE - 24);
          // let size = plaintext.len();
          // plaintext.resize(payload.len() - 4 - TAG_SIZE, 0);

          // println!("samples: {:?}", &buf.samples()[..960 / 4]);
          // println!("buf: {}", &buf.len());

          // let size = encoder.encode(&buf.samples()[..960 / 4], &mut plaintext)?;
          // println!("encoded: {:?}", &plaintext[..size]);

          // let mut debik = Vec::<i16>::new();
          // let mut decoded = MutSignals::try_from(&mut debik[..])?;
          // let decoded_size = opus_decoder.decode(Some(Packet::try_from(&plaintext[..size])?), decoded, false)?;
          // println!("decoded ({}): {:?}", decoded_size, &debik[..decoded_size]);

          // let mut nonce_bytes = Vec::new();
          // nonce_bytes.write_i32::<BigEndian>(nonce)?;
          // nonce_bytes.write_i32::<BigEndian>(0)?;
          // nonce_bytes.write_i32::<BigEndian>(0)?;
          // nonce_bytes.write_i32::<BigEndian>(0)?;
          // nonce_bytes.write_i32::<BigEndian>(0)?;
          // nonce_bytes.write_i32::<BigEndian>(0)?;

          let mut nonce_bytes = random::<[u8; 24]>();

          let ct = cipher.encrypt(GenericArray::from_slice(&nonce_bytes.to_vec()), &plaintext[..size])?;

          println!("Payload: {}, plaintext: {}, ct: {}", payload.len(), plaintext.len(), ct.len());
          let mut enn = Vec::new();
          {
            let mut c = Cursor::new(&mut enn);
            // c.write_all(&plaintext[..size])?;
            // c.write_all(&tag)?;
            c.write_all(&ct)?;
            c.write_all(&nonce_bytes)?;
            // c.write_i32::<BigEndian>(nonce)?;
          }
          nonce += 1;
          // println!("encrypted ({}): {:?}", plaintext[..size].len(), &plaintext[..size]);
          // println!("enn ({}): {:?}", enn.len(), enn);

          let decrypted = dcipher.decrypt(GenericArray::from_slice(&nonce_bytes.to_vec()), &enn[..size+TAG_SIZE]).unwrap();
          // println!("decrypted ({}): {:?}", decrypted.len(), decrypted);

          view.set_payload(&enn);

          println!("rtp_packet: {}", rtp_packet.len());
          // println!("rtp_packet: {:?}", rtp_packet);

          udp.send(&rtp_packet[..]).await?;

          let heartbeat_message = Message::Text(serde_json::to_string(&json!({
            "op": 1,
            "d": 12345
          })).unwrap());
          if let Err(error) = socket.send(heartbeat_message).await {
            println!("Error: {:?}", error);
          }

          let select_message = Message::Text(serde_json::to_string(&json!({
            "op": 5,
            "d": {
              "speaking": 1,
              "delay": 0,
              "ssrc": ready.ssrc
            }
          })).unwrap());
          if let Err(error) = socket.send(select_message).await {
            println!("Error: {:?}", error);
          }

          socket.flush().await?;

          thread::sleep(Duration::from_millis(100));
        }
      // }
      // Err(symphonia::core::errors::Error::IoError(_)) => {
      //   // The packet failed to decode due to an IO error, skip the packet.
      //   continue;
      // }
      // Err(symphonia::core::errors::Error::DecodeError(_)) => {
      //   // The packet failed to decode due to invalid data, skip the packet.
      //   continue;
      // }
      // Err(err) => {
      //   // An unrecoverable error occured, halt decoding.
      //   panic!("{}", err);
      // }
    // }
  // }

  Ok(())
}
