use symphonia::{
  core::{
    formats::FormatReader,
    codecs::{Decoder, CODEC_TYPE_NULL, DecoderOptions},
    probe::ProbeResult, audio::SampleBuffer
  },
  default::get_codecs
};

pub trait SampleProvider: Sync + Send {
  fn get_samples(&mut self, samples: &mut [i16]) -> usize;
}

pub struct SymphoniaSampleProvider {
  format: Box<dyn FormatReader>,
  track_id: u32,
  decoder: Box<dyn Decoder>
}

impl SymphoniaSampleProvider {
  pub fn new(probed: ProbeResult) -> Self {
    let format = probed.format;

    // Find the first audio track with a known (decodeable) codec.
    let track = format
      .tracks()
      .iter()
      .find(|it| it.codec_params.codec != CODEC_TYPE_NULL)
      .expect("no supported audio tracks");

    let track_id = track.id;

    let decoder = get_codecs()
      .make(&track.codec_params, &DecoderOptions::default())
      .expect("unsupported codec");

    SymphoniaSampleProvider { format, track_id, decoder }
  }
}

impl SampleProvider for SymphoniaSampleProvider {
  fn get_samples(&mut self, samples: &mut [i16]) -> usize {
    let mut sample_buf = None;

    loop {
      let packet = match self.format.next_packet() {
        Ok(packet) => packet,
        Err(symphonia::core::errors::Error::ResetRequired) => {
          // The track list has been changed. Re-examine it and create a new set of decoders,
          // then restart the decode loop. This is an advanced feature and it is not
          // unreasonable to consider this "the end." As of v0.5.0, the only usage of this is
          // for chained OGG physical streams.
          unimplemented!();
        }
        Err(err) => {
          // A unrecoverable error occured, halt decoding.
          panic!("{}", err);
        }
      };

      // Consume any new metadata that has been read since the last packet.
      while !self.format.metadata().is_latest() {
        // Pop the old head of the metadata queue.
        self.format.metadata().pop();

        // Consume the new metadata at the head of the metadata queue.
      }

      // If the packet does not belong to the selected track, skip over it.
      if packet.track_id() != self.track_id {
        continue;
      }

      // Decode the packet into audio samples.
      match self.decoder.decode(&packet) {
        Ok(buffer) => {
          // If this is the *first* decoded packet, create a sample buffer matching the
          // decoded audio buffer format.
          if sample_buf.is_none() {
            let spec = *buffer.spec();
            let duration = buffer.capacity() as u64;

            sample_buf = Some(SampleBuffer::<i16>::new(duration, spec));
          }

          // Copy the decoded audio buffer into the sample buffer in an interleaved format.
          if let Some(buf) = &mut sample_buf {
            buf.copy_interleaved_ref(buffer);

            // println!("Decoded {} samples", sample_count);

            let size = buf.samples().len();
            samples[..size].copy_from_slice(buf.samples());
            return size;
          }
        }
        Err(symphonia::core::errors::Error::IoError(_)) => {
          // The packet failed to decode due to an IO error, skip the packet.
          return 0;
        }
        Err(symphonia::core::errors::Error::DecodeError(_)) => {
          // The packet failed to decode due to invalid data, skip the packet.
          return 0;
        }
        Err(err) => {
          // An unrecoverable error occured, halt decoding.
          panic!("{}", err);
        }
      }
    }
  }
}
