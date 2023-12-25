use log::error;
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};
use tokio_stream::{
    wrappers::{ReceiverStream, WatchStream},
    Stream, StreamExt,
};

pub struct Register<T: Clone + Sync + Send + 'static> {
    rx: watch::Receiver<T>,
    tx: mpsc::Sender<T>,
    handle: JoinHandle<()>,
}

impl<T: Clone + Sync + Send + 'static> Drop for Register<T> {
    fn drop(&mut self) {
        self.handle.abort()
    }
}

impl<T: Clone + Sync + Send + 'static> Register<T> {
    pub fn new(initial_value: T) -> Self {
        let (watch_tx, rx) = watch::channel(initial_value);
        let (tx, mpsc_rx) = mpsc::channel(5);
        let mut receiver = ReceiverStream::new(mpsc_rx);
        let handle = tokio::spawn(async move {
            while let Some(v) = receiver.next().await {
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
}
