use std::net::IpAddr;

use log::error;
use thiserror::Error;
use tinkerforge_async::base58::Base58Error;
use tinkerforge_async::{
    error::TinkerforgeError,
    temperature_v2_bricklet::{
        TemperatureV2Bricklet, TEMPERATURE_V2_BRICKLET_STATUS_LED_CONFIG_OFF,
        TEMPERATURE_V2_BRICKLET_THRESHOLD_OPTION_OFF,
    },
};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

use crate::data::state::StateUpdateMessage;
use crate::data::Uid;
use crate::{
    data::registry::{EventRegistry, TemperatureKey},
    terminator::{TestamentReceiver, TestamentSender},
};

pub fn handle_temperature(
    bricklet: TemperatureV2Bricklet,
    event_registry: EventRegistry,
    temperature_key: TemperatureKey,
    uid: Uid,
    status_updater: mpsc::Sender<StateUpdateMessage>,
    ip_addr: IpAddr,
) -> TestamentSender {
    let (tx, rx) = TestamentSender::create();
    tokio::spawn(async move {
        if let Err(error) = temperature_task(
            bricklet,
            event_registry,
            temperature_key,
            rx,
            status_updater.clone(),
            ip_addr,
        )
        .await
        {
            error!("Error processing temperature: {error}");
        }
        status_updater
            .send(StateUpdateMessage::BrickletDisconnected {
                uid,
                endpoint: ip_addr,
            })
            .await
            .expect("Cannot send status update");
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
    termination_receiver: TestamentReceiver,
    status_updater: mpsc::Sender<StateUpdateMessage>,
    ip_addr: IpAddr,
) -> Result<(), TemperatureError> {
    bricklet
        .set_status_led_config(TEMPERATURE_V2_BRICKLET_STATUS_LED_CONFIG_OFF)
        .await?;
    bricklet
        .set_temperature_callback_configuration(
            500,
            true,
            TEMPERATURE_V2_BRICKLET_THRESHOLD_OPTION_OFF,
            20,
            20,
        )
        .await?;
    let id = bricklet.get_identity().await?;
    status_updater.send((ip_addr, id).try_into()?).await?;

    let mut stream = bricklet
        .get_temperature_callback_receiver()
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
    Ok(())
}
