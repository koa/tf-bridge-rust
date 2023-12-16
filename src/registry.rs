use std::collections::HashMap;
use std::future::Future;
use std::ops::DerefMut;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Local, Timelike};
use log::error;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tokio_stream::Stream;

use crate::register::Register;

pub trait TypedKey {
    type Value;
}

#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub enum ClockKey {
    MinuteClock,
    SecondClock,
}
impl TypedKey for ClockKey {
    type Value = DateTime<Local>;
}
#[async_trait]
pub trait KeyAccess<K: TypedKey<Value = V>, V: Clone + Sync + Send + 'static> {
    async fn register_access<
        'a,
        'b,
        F: Fn(&'a Register<DateTime<Local>>) -> FR + Send + 'a,
        R: 'b + Send,
        FR: Future<Output = R> + Send,
    >(
        &'a self,
        key: K,
        function: F,
    ) -> FR;
}

#[derive(Clone)]
pub struct EventRegistry {
    inner: Arc<Mutex<InnerEventRegistry>>,
}

/*
#[async_trait]
impl KeyAccess<ClockKey, DateTime<Local>> for EventRegistry {
    async fn register_access<
        'a,
        'b,
        F: Fn(&'a Register<DateTime<Local>>) -> FR + Send + 'a,
        R: 'b + Send,
        FR: Future<Output = R>,
    >(
        &'a self,
        key: ClockKey,
        function: F,
    ) -> FR {
        let guard = self.inner.lock().await;
        let result = function(&guard.clock_receiver);
        drop(guard);
        result
    }
}

 */

struct InnerEventRegistry {
    clock_receivers: HashMap<ClockKey, Register<DateTime<Local>>>,
}

impl EventRegistry {
    //pub async fn clock(&self) -> impl Stream<Item = DateTime<Local>> {
    // BroadcastStream::new(self.inner.lock().await.clock_receiver.resubscribe())
    //     .filter_map(|e| e.ok())
    //}
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(InnerEventRegistry::new())),
        }
    }
    pub async fn clock(&self, key: ClockKey) -> impl Stream<Item = DateTime<Local>> {
        let mut guard = self.inner.lock().await;
        guard
            .deref_mut()
            .clock_receivers
            .entry(key)
            .or_insert_with_key(|clock_key| {
                let step_size = match clock_key {
                    ClockKey::MinuteClock => Duration::from_secs(60).as_millis() as u32,
                    ClockKey::SecondClock => Duration::from_secs(1).as_millis() as u32,
                };

                let clock_receiver = Register::new(Local::now());
                let sender = clock_receiver.sender();
                tokio::spawn(async move {
                    loop {
                        let time = Local::now();
                        match sender.send(time).await {
                            Ok(()) => {}
                            Err(error) => {
                                error!("Cannot send wall clock: {error}")
                            }
                        }
                        let wait_time = step_size
                            - (time.timestamp_subsec_millis() + time.second() * 1000) % step_size;
                        sleep(Duration::from_millis(wait_time as u64)).await;
                    }
                });
                clock_receiver
            })
            .stream()
            .await
    }
}

impl InnerEventRegistry {
    fn new() -> Self {
        Self {
            clock_receivers: Default::default(),
        }
    }
}
