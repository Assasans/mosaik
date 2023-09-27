use std::sync::{Arc, RwLock};
use tokio::sync::watch::{self, Receiver, Sender};

// TODO(Assasans): Use watch::[Sender/Receiver]<T>?
pub struct StateFlow<T> {
  inner: Arc<RwLock<T>>,
  sender: Sender<()>,
  receiver: Receiver<()>
}

impl<T: Clone> StateFlow<T> {
  pub fn new(value: T) -> Self {
    let (sender, receiver) = watch::channel(());
    Self {
      inner: Arc::new(RwLock::new(value)),
      sender,
      receiver
    }
  }

  pub fn set(&self, value: T) {
    *self.inner.write().unwrap() = value;
    self.sender.send(()).unwrap() // It is not possible that [receiver] will be dropped
  }

  pub async fn await_change(&self) -> T {
    let mut receiver = self.receiver.clone();
    receiver.borrow_and_update();
    receiver.changed().await.unwrap(); // It is not possible that [receiver] will be dropped

    self.get()
  }

  pub async fn wait_for(&self, block: impl Fn(&T) -> bool) -> T {
    let mut receiver = self.receiver.clone();
    receiver.borrow_and_update();

    // Check if current value matches
    let value = self.get();
    if block(&value) {
      return value;
    }

    loop {
      receiver.changed().await.unwrap(); // It is not possible that [receiver] will be dropped

      let value = self.get();
      if block(&value) {
        return value;
      }
    }
  }

  pub fn get(&self) -> T {
    self.inner.read().unwrap().clone()
  }
}
