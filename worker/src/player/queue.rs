use std::fmt::{Debug, Formatter};
use std::sync::{Arc, RwLock, Weak};
use std::sync::atomic::{AtomicUsize, Ordering};
use crate::player::track::Track;

#[derive(Debug)]
pub struct Queue {
  pub tracks: RwLock<Vec<Arc<Track>>>,
  position: AtomicUsize,
  pub mode: RwLock<Box<dyn PlayMode>>
}

impl Queue {
  pub fn new() -> Arc<Self> {
    let me = Self {
      tracks: RwLock::new(Vec::new()),
      position: AtomicUsize::new(0),
      mode: RwLock::new(Box::new(UninitializedPlayMode {}))
    };
    let me = Arc::new(me);
    me.set_mode(Box::new(NormalPlayMode::new(Arc::downgrade(&me))));
    me
  }

  pub fn set_mode(&self, mode: Box<dyn PlayMode>) {
    *self.mode.write().unwrap() = mode;
  }

  pub fn set_position(&self, position: usize) {
    self.position.store(position, Ordering::Relaxed);
  }

  pub fn position(&self) -> usize {
    self.position.load(Ordering::Relaxed)
  }

  pub fn len(&self) -> usize {
    self.tracks.read().unwrap().len()
  }

  pub fn get_current(&self) -> Weak<Track> {
    let tracks = self.tracks.read().unwrap();
    Arc::downgrade(tracks.get(self.position()).unwrap())
  }

  pub fn push(&self, track: Track) -> (Arc<Track>, usize) {
    let mut tracks = self.tracks.write().unwrap();
    let track = Arc::new(track);
    tracks.push(track.clone());
    (track, tracks.len() - 1)
  }
}

pub trait PlayMode: Send + Sync + Debug {
  /// Returns the index of a track with a relative position within the queue.
  ///
  /// If this is a user initiated seek - set [force] to [true].
  /// If this is an automatic seek (next track in queue) - set [force] to false.
  fn seek(&self, offset: isize, force: bool) -> Option<usize>;
}

pub struct UninitializedPlayMode;

impl PlayMode for UninitializedPlayMode {
  fn seek(&self, offset: isize, force: bool) -> Option<usize> {
    unreachable!()
  }
}

impl Debug for UninitializedPlayMode {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("UninitializedPlayMode").finish()
  }
}

pub struct NormalPlayMode {
  queue: Weak<Queue>
}

impl NormalPlayMode {
  pub fn new(queue: Weak<Queue>) -> Self {
    Self { queue }
  }
}

impl PlayMode for NormalPlayMode {
  fn seek(&self, offset: isize, force: bool) -> Option<usize> {
    let queue = match self.queue.upgrade() {
      Some(queue) => queue,
      None => unreachable!("queue droppped")
    };

    let range = 0..queue.len();
    let position = (queue.position() as isize + offset) as usize;
    if range.contains(&position) {
      Some(position)
    } else {
      None
    }
  }
}

impl Debug for NormalPlayMode {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("NormalPlayMode").finish()
  }
}

pub struct LoopPlayMode {
  queue: Weak<Queue>
}

impl LoopPlayMode {
  pub fn new(queue: Weak<Queue>) -> Self {
    Self { queue }
  }
}

impl PlayMode for LoopPlayMode {
  fn seek(&self, offset: isize, force: bool) -> Option<usize> {
    assert_eq!(offset, 1); // TODO(Assasans): Not implemented

    let queue = match self.queue.upgrade() {
      Some(queue) => queue,
      None => unreachable!("queue droppped")
    };

    if queue.len() < 1 {
      return None;
    }

    if queue.position() < queue.len() {
      Some(queue.position() + 1)
    } else {
      Some(0)
    }
  }
}

impl Debug for LoopPlayMode {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("LoopPlayMode").finish()
  }
}
