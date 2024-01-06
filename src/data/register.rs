use log::error;
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};
use tokio_stream::{
    wrappers::{ReceiverStream, WatchStream},
    Stream, StreamExt,
};

pub struct Register<T: Clone + Sync + Send + 'static + PartialEq> {
    rx: watch::Receiver<T>,
    tx: mpsc::Sender<T>,
    handle: JoinHandle<()>,
}

impl<T: Clone + Sync + Send + 'static + PartialEq> Drop for Register<T> {
    fn drop(&mut self) {
        self.handle.abort()
    }
}

impl<T: Clone + Sync + Send + 'static + PartialEq> Register<T> {
    pub fn new(initial_value: T) -> Self {
        let (watch_tx, rx) = watch::channel(initial_value);
        let (tx, mpsc_rx) = mpsc::channel::<T>(5);
        let mut receiver = ReceiverStream::new(mpsc_rx);
        let handle = tokio::spawn(async move {
            let mut last_value = None;
            while let Some(v) = receiver.next().await {
                let current_value = Some(v.clone());
                if last_value == current_value {
                    continue;
                }
                last_value = current_value;
                match watch_tx.send(v) {
                    Ok(_) => {}
                    Err(error) => {
                        error!("Cannot send message {error}");
                    }
                }
            }
        });
        Self { rx, tx, handle }
    }
    pub async fn stream(&self) -> impl Stream<Item = T> {
        WatchStream::new(self.rx.clone())
    }
    pub fn sender(&self) -> mpsc::Sender<T> {
        self.tx.clone()
    }
    pub fn current_value(&self) -> T {
        self.rx.borrow().clone()
    }
}
impl<T: Clone + Sync + Send + Default + 'static + PartialEq> Default for Register<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}
