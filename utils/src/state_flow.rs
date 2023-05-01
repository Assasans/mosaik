use std::sync::Arc;
use anyhow::Result;
use tokio::sync::{watch, RwLock};

// TODO(Assasans): Use watch::[Sender/Receiver]<T>?
pub struct StateFlow<T> {
  inner: Arc<RwLock<T>>,
  sender: watch::Sender<()>,
  receiver: watch::Receiver<()>
}

impl<T> StateFlow<T> {
  pub fn new(val: T) -> Self {
    let (sender, receiver) = watch::channel(());
    Self {
      inner: Arc::new(RwLock::new(val)),
      sender,
      receiver,
    }
  }

  pub async fn set(&self, val: T) -> Result<()> {
    *self.inner.write().await = val;
    self.sender.send(())?;

    Ok(())
  }

  pub async fn await_change(&self) -> Result<T> where T: Clone {
    let mut receiver = self.receiver.clone();
    receiver.changed().await?;

    Ok(self.get().await)
  }

  pub async fn get(&self) -> T where T: Clone {
    self.inner.read().await.clone()
  }
}
