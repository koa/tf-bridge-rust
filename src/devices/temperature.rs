use log::error;
use thiserror::Error;
use tinkerforge_async::{
    base58::Base58Error,
    error::TinkerforgeError,
    temperature_v_2::{
        SetTemperatureCallbackConfigurationRequest, TemperatureV2Bricklet, ThresholdOption,
    },
};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

use crate::terminator::LifeLineEnd;
use crate::{
    data::{
        registry::{EventRegistry, TemperatureKey},
        state::StateUpdateMessage,
    },
    terminator::{TestamentReceiver, TestamentSender},
};

pub fn handle_temperature(
    bricklet: TemperatureV2Bricklet,
    event_registry: EventRegistry,
    temperature_key: TemperatureKey,
) -> LifeLineEnd {
    let (tx, rx) = LifeLineEnd::create();
    tokio::spawn(async move {
        if let Err(error) = temperature_task(bricklet, event_registry, temperature_key, rx).await {
            error!("Error processing temperature: {error}");
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
}

async fn temperature_task(
    mut bricklet: TemperatureV2Bricklet,
    event_registry: EventRegistry,
    temperature_key: TemperatureKey,
    termination_receiver: LifeLineEnd,
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

    let mut stream = bricklet
        .temperature_stream()
        .await
        .map(|t| TemperatureEvent::Temperature(t as f32 / 100.0))
        .merge(termination_receiver.send_on_terminate(TemperatureEvent::Closed));
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
        }
    }
    drop(termination_receiver);
    Ok(())
}
