use log::info;
use tokio::sync::mpsc;
use tokio::task::{AbortHandle, JoinHandle};

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
}

pub struct JoinHandleTerminator<T>(JoinHandle<T>);

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
