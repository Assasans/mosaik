use std::fmt::{Debug, Formatter};
use std::io;
use anyhow::Result;
use rubato::{Resampler, FftFixedIn};
use symphonia::core::{
  formats::{FormatReader, FormatOptions},
  codecs::{Decoder, CODEC_TYPE_NULL, DecoderOptions},
  probe::{ProbeResult, Hint},
  audio::{SampleBuffer, SignalSpec},
  meta::MetadataOptions,
  io::{MediaSourceStream, MediaSource}
};
use tracing::field::debug;
use tracing::{debug, error, info};

use voice::provider::SampleProvider;

pub struct SymphoniaSampleProvider {
  format: Box<dyn FormatReader>,
  track_id: u32,
  decoder: Box<dyn Decoder>,
  resampler: Option<FftFixedIn<f32>>,
  spec: Option<SignalSpec>,
  sample_buf: Option<SampleBuffer<f32>>,
  resample_out: Option<Vec<Vec<f32>>>,
  resample_interleaved_out: Option<Vec<f32>>
}

impl Debug for SymphoniaSampleProvider {
  fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
    formatter.debug_struct("SymphoniaSampleProvider")
      .finish()
  }
}

fn planar1d_to_planar2d<T>(input: &[T], channels: usize) -> Vec<&[T]> {
  let chunk_size = input.len() / channels;
  let mut result = Vec::with_capacity(channels);
  for channel in 0..channels {
    let start = channel * chunk_size;
    let end = start + chunk_size;
    result.push(&input[start..end]);
  }

  result
}

fn planar_to_interleave<T, F>(input: &[F], output: &mut [T], frames: usize) -> usize where T: Copy + Default, F: AsRef<[T]> {
  let channels = input.len();

  for frame in 0..frames {
    for channel in 0..channels {
      let samples = input[channel].as_ref();
      output[frame * channels + channel] = samples[frame];
    }
  }
  channels * frames
}

fn vectors_to_slices<T>(vectors: &[Vec<T>]) -> Vec<&[T]> {
  vectors.iter().map(Vec::as_slice).collect()
}

impl SymphoniaSampleProvider {
  /// Creates new [`SymphoniaSampleProvider`] from [`MediaSource`] and [`Hint`]
  pub fn new_from_source(source: Box<dyn MediaSource>, hint: Hint) -> Result<Self> {
    let stream = MediaSourceStream::new(source, Default::default());
    let probed = symphonia::default::get_probe()
      .format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default())
      .expect("unsupported format");

    Ok(SymphoniaSampleProvider::new(probed))
  }

  /// Creates new [`SymphoniaSampleProvider`] from [`ProbeResult`]
  pub fn new(probed: ProbeResult) -> Self {
    let format = probed.format;

    // Find the first audio track with a known (decodeable) codec.
    let track = format
      .tracks()
      .iter()
      .find(|it| it.codec_params.codec != CODEC_TYPE_NULL)
      .expect("no supported audio tracks");

    let track_id = track.id;

    let decoder = symphonia::default::get_codecs()
      .make(&track.codec_params, &DecoderOptions::default())
      .expect("unsupported codec");

    SymphoniaSampleProvider {
      format,
      track_id,
      decoder,
      resampler: None,
      spec: None,
      sample_buf: None,
      resample_out: None,
      resample_interleaved_out: None
    }
  }

  fn process_samples(&mut self) -> Result<&[f32]> {
    let input = self.sample_buf.as_ref().unwrap().samples();
    let output = self.resample_interleaved_out.as_mut().unwrap();

    // debug!("Input zeroes: {}", input.iter().filter(|&n| n.abs() < 0.00001).count());

    let spec = self.spec.as_ref().unwrap();

    let resampler = self.resampler.as_mut().unwrap();
    let resample_out = self.resample_out.as_mut().unwrap();

    let frames_in = planar1d_to_planar2d(input, spec.channels.count());
    let (planar, planar_size) = if spec.rate != 48000 {
      let missing = resampler.input_frames_next() - input.len() / spec.channels.count();
      if missing > 0 {
        // TODO(Assasans): Wait from next packet?
        error!("missing {} samples", missing);
        return Ok(&output[0..0]);
      }

      let out_size = resampler.output_frames_next();
      resampler.process_into_buffer(&frames_in, resample_out, None).unwrap();
      // debug!("Resampled {} -> {} samples", input.len(), out_size * spec.channels.count());
      (vectors_to_slices(resample_out), out_size)
    } else {
      // Native sample rate, no need to resample
      (frames_in, input.len() / spec.channels.count())
    };
    let interleaved_size = planar_to_interleave(&planar, output, planar_size);

    // debug!("Output zeroes: {}", output.iter().filter(|&n| n.abs() < 0.00001).count());

    Ok(&output[..interleaved_size])
  }
}

impl SampleProvider for SymphoniaSampleProvider {
  fn get_samples(&mut self, out: &mut [f32]) -> usize {
    loop {
      let packet = match self.format.next_packet() {
        Ok(packet) => packet,
        Err(symphonia::core::errors::Error::IoError(error)) if error.kind() == io::ErrorKind::UnexpectedEof => {
          return 0;
        },
        Err(symphonia::core::errors::Error::ResetRequired) => {
          // The track list has been changed. Re-examine it and create a new set of decoders,
          // then restart the decode loop. This is an advanced feature and it is not
          // unreasonable to consider this "the end." As of v0.5.0, the only usage of this is
          // for chained OGG physical streams.
          unimplemented!();
        },
        Err(err) => {
          // A unrecoverable error occurred, halt decoding.
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
          if self.sample_buf.is_none() {
            let spec = *buffer.spec();
            let duration = buffer.capacity() as u64;

            self.sample_buf = Some(SampleBuffer::<f32>::new(duration, spec));
            self.spec = Some(spec);
            let resampler = self.resampler.insert(FftFixedIn::<f32>::new(
              spec.rate as usize,
              48000,
              buffer.capacity(),
              2,
              spec.channels.count()
            ).unwrap());
            self.resample_out = Some(resampler.output_buffer_allocate());
            self.resample_interleaved_out = Some(vec![0f32; resampler.output_frames_max() * spec.channels.count()]);
          }

          // Copy the decoded audio buffer into the sample buffer in an interleaved format.
          if let Some(buf) = self.sample_buf.as_mut() {
            // buf.copy_interleaved_ref(buffer);
            buf.copy_planar_ref(buffer);

            // println!("Decoded {} samples", sample_count);

            let output = self.process_samples().unwrap();

            out[..output.len()].copy_from_slice(&output);
            return output.len();
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
          // An unrecoverable error occurred, halt decoding.
          panic!("{}", err);
        }
      }
    }
  }
}
