use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Local, Timelike};
use log::{error, info};
use tokio::{
    sync::{
        broadcast::{self, Receiver},
        Mutex,
    },
    time::sleep,
};
use tokio_stream::{wrappers::BroadcastStream, Stream, StreamExt};

#[derive(Clone)]
pub struct EventRegistry {
    inner: Arc<Mutex<InnerEventRegistry>>,
}

struct InnerEventRegistry {
    clock_receiver: Receiver<DateTime<Local>>,
}

impl EventRegistry {
    pub async fn clock(&self) -> impl Stream<Item = DateTime<Local>> {
        BroadcastStream::new(self.inner.lock().await.clock_receiver.resubscribe())
            .filter_map(|e| e.ok())
    }
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(InnerEventRegistry::new())),
        }
    }
}

impl InnerEventRegistry {
    fn new() -> Self {
        let (tx, clock_receiver) = broadcast::channel(2);
        tokio::spawn(async move {
            loop {
                let time = Local::now();
                match tx.send(time) {
                    Ok(count) => {
                        info!("Sent clock to {count}")
                    }
                    Err(error) => {
                        error!("Cannot send wall clock: {error}")
                    }
                }
                let wait_time = 60000 - (time.timestamp_subsec_millis() + time.second() * 1000);
                sleep(Duration::from_millis(wait_time as u64)).await;
            }
        });

        Self { clock_receiver }
    }
}
