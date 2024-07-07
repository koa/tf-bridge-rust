use std::time::Duration;

use log::error;
use thiserror::Error;
use tinkerforge_async::base58::Uid;
use tinkerforge_async::{
    base58::Base58Error,
    error::TinkerforgeError,
    temperature_v_2::{
        SetTemperatureCallbackConfigurationRequest, TemperatureV2Bricklet, ThresholdOption,
    },
};
use tokio::{
    sync::mpsc::{self, Sender},
    task::JoinHandle,
};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};

use crate::{
    data::{
        registry::{EventRegistry, TemperatureKey},
        state::{SpitfpErrorCounters, StateUpdateMessage},
    },
    terminator::LifeLineEnd,
    util::{self, TimerHandle},
};

pub fn handle_temperature(
    bricklet: TemperatureV2Bricklet,
    event_registry: EventRegistry,
    temperature_key: TemperatureKey,
    status_updater: Sender<StateUpdateMessage>,
) -> LifeLineEnd {
    let (tx, rx) = LifeLineEnd::create();
    let uid = bricklet.uid();
    tokio::spawn(async move {
        if let Err(error) = temperature_task(
            bricklet,
            event_registry,
            temperature_key,
            rx,
            &status_updater,
        )
        .await
        {
            error!("Error processing temperature: {error}");
            status_updater
                .send(StateUpdateMessage::CommunicationFailed { uid })
                .await
                .expect("Status handler down");
        }
    });
    tx
}

#[derive(Error, Debug)]
enum TemperatureError {
    #[error("Tinkerforge error: {0}")]
    Tinkerforge(#[from] TinkerforgeError),
    #[error("Send error: {0}")]
    SendError(#[from] mpsc::error::SendError<f32>),
    #[error("Cannot update state {0}")]
    UpdateState(#[from] mpsc::error::SendError<StateUpdateMessage>),
    #[error("Cannot parse UID {0}")]
    Uid(#[from] Base58Error),
}

enum TemperatureEvent {
    Temperature(f32),
    Closed,
    CollectMetrics,
}

async fn temperature_task(
    mut bricklet: TemperatureV2Bricklet,
    event_registry: EventRegistry,
    temperature_key: TemperatureKey,
    termination_receiver: LifeLineEnd,
    status_updater: &Sender<StateUpdateMessage>,
) -> Result<(), TemperatureError> {
    bricklet
        .set_temperature_callback_configuration(SetTemperatureCallbackConfigurationRequest {
            period: 10000,
            value_has_to_change: true,
            option: ThresholdOption::Off,
            min: 20,
            max: 20,
        })
        .await?;
    let (tx, rx) = mpsc::channel(2);
    let uid = bricklet.uid();

    let mut metrics_timer: TimerHandle =
        start_metrics_timer(tx.clone(), Duration::from_secs(1), uid).into();

    let mut stream = bricklet
        .temperature_stream()
        .await
        .map(|t| TemperatureEvent::Temperature(t as f32 / 100.0))
        .merge(termination_receiver.send_on_terminate(TemperatureEvent::Closed))
        .merge(ReceiverStream::new(rx));
    let sender = event_registry.temperature_sender(temperature_key).await;
    sender
        .send(bricklet.get_temperature().await? as f32 / 100.0)
        .await?;
    while let Some(event) = stream.next().await {
        match event {
            TemperatureEvent::Temperature(t) => {
                sender.send(t).await?;
            }
            TemperatureEvent::Closed => break,
            TemperatureEvent::CollectMetrics => {
                let counters = bricklet.get_spitfp_error_count().await?;
                status_updater
                    .send(StateUpdateMessage::SpitfpMetrics {
                        uid,
                        port: None,
                        counters: SpitfpErrorCounters {
                            error_count_ack_checksum: counters.error_count_ack_checksum,
                            error_count_message_checksum: counters.error_count_message_checksum,
                            error_count_frame: counters.error_count_frame,
                            error_count_overflow: counters.error_count_overflow,
                        },
                    })
                    .await?;
                metrics_timer.restart(start_metrics_timer(
                    tx.clone(),
                    Duration::from_secs(3600),
                    uid,
                ));
            }
        }
    }
    drop(termination_receiver);
    Ok(())
}

fn start_metrics_timer(
    tx: Sender<TemperatureEvent>,
    duration: Duration,
    uid: Uid,
) -> JoinHandle<()> {
    util::send_delayed_event(TemperatureEvent::CollectMetrics, duration, tx, move |e| {
        error!("Cannot collect metrics on temperature sensor {uid}: {e}")
    })
}
