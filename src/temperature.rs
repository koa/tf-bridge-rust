use log::error;
use thiserror::Error;
use tinkerforge_async::error::TinkerforgeError;
use tinkerforge_async::temperature_v2_bricklet::TemperatureV2Bricklet;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Receiver;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use crate::registry::{EventRegistry, TemperatureKey};

pub fn handle_temperature(
    bricklet: TemperatureV2Bricklet,
    event_registry: EventRegistry,
    temperature_key: TemperatureKey,
) -> mpsc::Sender<()> {
    let (tx, rx) = mpsc::channel(1);
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
}
enum TemperatureEvent {
    Temperature(f32),
    Closed,
}

async fn temperature_task(
    mut bricklet: TemperatureV2Bricklet,
    event_registry: EventRegistry,
    temperature_key: TemperatureKey,
    termination_receiver: Receiver<()>,
) -> Result<(), TemperatureError> {
    let mut stream = bricklet
        .get_temperature_callback_receiver()
        .await
        .map(|t| TemperatureEvent::Temperature(t as f32 / 100.0))
        .merge(ReceiverStream::new(termination_receiver).map(|_| TemperatureEvent::Closed));
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
    Ok(())
}
