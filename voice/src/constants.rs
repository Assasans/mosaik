use std::time::Duration;

pub const CHANNEL_COUNT: usize = 2;
pub const SAMPLE_RATE: usize = 48000;
pub const CHUNK_DURATION: Duration = Duration::from_millis(20);
pub const TIMESTAMP_STEP: usize = SAMPLE_RATE / (1000 / CHUNK_DURATION.as_millis() as usize);

pub const OPUS_SILENCE_FRAME: [u8; 3] = [0xF8, 0xFF, 0xFE];
pub const OPUS_SILENCE_FRAMES: u8 = 5;
