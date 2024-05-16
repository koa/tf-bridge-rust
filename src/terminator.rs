use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use log::info;
use tokio::{
    sync::{mpsc, watch},
    task::{AbortHandle, JoinHandle},
};
use tokio_stream::{Stream, StreamExt, wrappers::WatchStream};

pub struct TestamentSender(Option<watch::Sender<Option<()>>>);

#[derive(Clone)]
pub struct TestamentReceiver(watch::Receiver<Option<()>>);

pub struct LifeLineEnd {
    #[allow(unused)]
    sender: TestamentSender,
    receiver: TestamentReceiver,
    is_alive: Arc<AtomicBool>,
}

impl TestamentReceiver {
    pub fn update_on_terminate<R: Send + 'static>(mut self, message: R, sender: mpsc::Sender<R>) {
        tokio::spawn(async move {
            self.0
                .wait_for(|o| o.is_some())
                .await
                .expect("Cannot wait for terminate");
            sender.send(message).await.expect("Cannot send termination");
        });
    }
    pub fn send_on_terminate<R>(self, value: R) -> impl Stream<Item = R> + Unpin {
        let mut value = Some(value);
        WatchStream::new(self.0)
            .filter_map(|o| o)
            .filter_map(move |_| value.take())
    }
}

impl TestamentSender {
    pub fn create() -> (TestamentSender, TestamentReceiver) {
        let (tx, rx) = watch::channel(None);
        (TestamentSender(Some(tx)), TestamentReceiver(rx))
    }
}

impl Drop for TestamentSender {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            if let Err(error) = sender.send(Some(())) {
                info!("Error on cleanup: {error}, ignoring");
            }
        }
    }
}

impl LifeLineEnd {
    pub fn create() -> (LifeLineEnd, LifeLineEnd) {
        let (tx1, rx1) = TestamentSender::create();
        let (tx2, rx2) = TestamentSender::create();
        (
            Self::create_single_end(tx1, rx2),
            Self::create_single_end(tx2, rx1),
        )
    }

    fn create_single_end(tx: TestamentSender, rx: TestamentReceiver) -> LifeLineEnd {
        let is_alive = Arc::new(AtomicBool::new(true));
        let mut receiver = rx.clone().send_on_terminate(());
        let is_alive_writer = is_alive.clone();
        tokio::spawn(async move {
            receiver.next().await;
            is_alive_writer.store(false, Ordering::Relaxed);
        });
        LifeLineEnd {
            sender: tx,
            receiver: rx,
            is_alive,
        }
    }
    pub fn is_alive(&self) -> bool {
        self.is_alive.load(Ordering::Relaxed)
    }
    pub fn send_on_terminate<R>(&self, value: R) -> impl Stream<Item = R> + Unpin {
        self.receiver.clone().send_on_terminate(value)
    }
    pub fn update_on_terminate<R: Send + 'static>(&self, message: R, sender: mpsc::Sender<R>) {
        self.receiver.clone().update_on_terminate(message, sender);
    }
}

pub struct JoinHandleTerminator<T>(JoinHandle<T>);

impl<T> From<JoinHandle<T>> for JoinHandleTerminator<T> {
    fn from(value: JoinHandle<T>) -> Self {
        Self(value)
    }
}

impl<T> JoinHandleTerminator<T> {
    pub fn new(handle: JoinHandle<T>) -> Self {
        Self(handle)
    }
}

impl<T> Drop for JoinHandleTerminator<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub struct AbortHandleTerminator(AbortHandle);

impl AbortHandleTerminator {
    pub fn new(handle: AbortHandle) -> Self {
        Self(handle)
    }
}

impl Drop for AbortHandleTerminator {
    fn drop(&mut self) {
        self.0.abort();
    }
}
