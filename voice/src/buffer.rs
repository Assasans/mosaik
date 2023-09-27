use std::cmp::min;
use anyhow::Result;
use std::sync::atomic::{AtomicUsize, Ordering};
use ringbuf::{HeapConsumer, HeapProducer, HeapRb};
use tokio::sync::{Mutex, watch};
use tokio::sync::watch::{Receiver, Sender};
use tracing::{debug, trace};
use utils::state_flow::StateFlow;

pub struct SampleBuffer<T> {
  pub low_threshold: usize,
  pub high_threshold: usize,
  is_corked: StateFlow<bool>,
  write_performed: (Sender<()>, Receiver<()>),

  producer: Mutex<HeapProducer<T>>,
  consumer: Mutex<HeapConsumer<T>>,

  length: AtomicUsize
}

impl<T: Copy> SampleBuffer<T> {
  pub fn new(capacity: usize, low_threshold: usize, high_threshold: usize) -> Self {
    assert!(low_threshold <= high_threshold);
    assert!(low_threshold <= capacity);
    assert!(high_threshold <= capacity);

    let buffer = HeapRb::<T>::new(capacity);
    let (producer, consumer) = buffer.split();

    Self {
      low_threshold,
      high_threshold,
      is_corked: StateFlow::new(false),
      write_performed: watch::channel(()),

      producer: Mutex::new(producer),
      consumer: Mutex::new(consumer),

      length: AtomicUsize::new(0)
    }
  }

  pub fn len(&self) -> usize {
    self.length.load(Ordering::Relaxed)
  }

  pub async fn wait_for(&self, size: usize) -> Result<()> {
    trace!("waiting for at least {} samples to be available...", size);
    loop {
      let length = self.len();
      if length >= size {
        break;
      }

      let mut write_performed = self.write_performed.1.clone();
      write_performed.borrow_and_update();
      debug!("insufficient buffer length: {} < {}", length, size);
      write_performed.changed().await.unwrap(); // It is not possible that [self.write_performed.0] will be dropped
    }

    Ok(())
  }

  pub async fn write(&self, data: &[T]) -> Result<()> {
    trace!("writing {} samples", data.len());
    self.is_corked.wait_for(|it| *it == false).await;

    let mut producer = self.producer.lock().await;
    let mut written = 0;
    while written < data.len() {
      let end = min(written + producer.free_len(), data.len());
      producer.push_slice(&data[written..end]);
      let len = producer.len();
      self.length.store(len, Ordering::Relaxed);
      self.write_performed.0.send(())?;
      trace!("written {written}..{end} ({}) samples", end - written);
      written = end;

      if len >= self.high_threshold {
        self.is_corked.set(true);
        debug!("write: buffer corked: {} >= {}", len, self.high_threshold);
      }

      if self.is_corked.get() {
        self.is_corked.wait_for(|it| *it == false).await;
        trace!("write: buffer uncorked: {} <= {}", producer.len(), self.low_threshold);
      }
    }

    Ok(())
  }

  pub async fn read(&self, data: &mut [T]) -> Result<()> {
    trace!("reading {} samples", data.len());
    self.wait_for(data.len()).await?;

    let mut consumer = self.consumer.lock().await;
    assert!(consumer.len() >= data.len());
    consumer.pop_slice(data);
    self.length.fetch_sub(data.len(), Ordering::Relaxed);

    if consumer.len() <= self.low_threshold && self.is_corked.get() {
      self.is_corked.set(false);
      debug!("read: buffer uncorked: {} <= {}", consumer.len(), self.low_threshold);
    }

    Ok(())
  }

  pub async fn flush(&self) -> Vec<T> {
    let mut consumer = self.consumer.lock().await;

    let data = consumer.pop_iter().collect::<Vec<T>>();
    self.length.store(0, Ordering::Relaxed);
    self.is_corked.set(false);
    debug!("flush: buffer uncorked: {} <= {}", consumer.len(), self.low_threshold);

    data
  }

  pub async fn clear(&self) -> () {
    let mut consumer = self.consumer.lock().await;
    consumer.clear();

    self.length.store(0, Ordering::Relaxed);
    self.is_corked.set(false);
    debug!("clear: buffer uncorked: {} <= {}", consumer.len(), self.low_threshold);
  }
}
