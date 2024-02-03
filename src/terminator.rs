use log::info;
use tokio::sync::watch;
use tokio::task::{AbortHandle, JoinHandle};
use tokio_stream::wrappers::WatchStream;
use tokio_stream::{Stream, StreamExt};

/*
pub struct DeviceThreadTerminator(Option<mpsc::Sender<()>>);

impl DeviceThreadTerminator {
    pub fn new(sender: mpsc::Sender<()>) -> Self {
        Self(Some(sender))
    }
}

impl Drop for DeviceThreadTerminator {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            tokio::spawn(async move {
                if let Err(error) = sender.send(()).await {
                    info!("Error on cleanup: {error}, ignoring")
                }
            });
        }
    }
}*/

pub struct TestamentSender(Option<watch::Sender<Option<()>>>);
#[derive(Clone)]
pub struct TestamentReceiver(watch::Receiver<Option<()>>);

impl TestamentReceiver {
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
    pub(crate) fn is_finished(&self) -> bool {
        self.0.is_finished()
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
