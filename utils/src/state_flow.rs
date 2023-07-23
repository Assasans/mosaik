use std::sync::{Arc, RwLock};
use tokio::sync::watch::{self, Receiver, Sender};
use tokio::sync::watch::error::{RecvError, SendError};

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

  pub fn set(&self, value: T) -> Result<(), SendError<()>> {
    *self.inner.write().unwrap() = value;
    self.sender.send(())
  }

  pub async fn await_change(&self) -> Result<T, RecvError> {
    let mut receiver = self.receiver.clone();
    receiver.borrow_and_update();
    receiver.changed().await?;

    Ok(self.get())
  }

  pub async fn wait_for(&self, block: impl Fn(&T) -> bool) -> Result<T, RecvError> {
    let mut receiver = self.receiver.clone();
    receiver.borrow_and_update();

    loop {
      receiver.changed().await?;

      let value = self.get();
      if block(&value) {
        return Ok(value);
      }
    }
  }

  pub fn get(&self) -> T {
    self.inner.read().unwrap().clone()
  }
}
