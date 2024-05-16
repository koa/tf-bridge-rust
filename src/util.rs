use std::future::Future;
use std::time::Duration;

use tokio::sync::mpsc::error::SendError;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tokio_stream::{empty, Empty, Stream};
use tokio_util::either::Either;

pub async fn optional_stream<ISF: Future<Output = IS>, IS: Stream<Item = T>, T>(
    is: Option<ISF>,
) -> Either<IS, Empty<T>> {
    if let Some(is) = is {
        Either::Left(is.await)
    } else {
        Either::Right(empty::<T>())
    }
}

pub fn kelvin_2_mireds(temp: u16) -> u16 {
    (1000000 / temp as u32) as u16
}

#[derive(Default, Debug)]
pub struct TimerHandle(Option<JoinHandle<()>>);

impl From<JoinHandle<()>> for TimerHandle {
    fn from(value: JoinHandle<()>) -> Self {
        Self(Some(value))
    }
}

impl TimerHandle {
    pub fn restart(&mut self, handle: JoinHandle<()>) {
        if let Some(old_timer) = self.0.replace(handle) {
            old_timer.abort();
        }
    }
}

impl Drop for TimerHandle {
    fn drop(&mut self) {
        if let Some(handle) = &self.0 {
            handle.abort();
        }
    }
}

pub fn send_delayed_event<E: Fn(SendError<M>) + Send + 'static, M: Send + 'static>(
    event: M,
    duration: Duration,
    tx: Sender<M>,
    error_handler: E,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        sleep(duration).await;
        if let Err(e) = tx.send(event).await {
            error_handler(e);
        }
    })
}
