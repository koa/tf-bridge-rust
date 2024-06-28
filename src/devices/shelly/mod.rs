use std::{net::IpAddr, sync::Arc, time::Duration};

use jsonrpsee::{
    client_transport::{
        ws::{Url, WsTransportClientBuilder},
        ws::WsHandshakeError,
    },
    core::client::{Client, ClientBuilder},
};
use jsonrpsee::core::{client, ClientError};
use log::{error, info};
use thiserror::Error;
use tokio::{sync::mpsc, task, time::sleep};

use crate::{
    data::{
        registry::EventRegistry, settings::Shelly, state::StateUpdateMessage, wiring::ShellyDevices,
    },
    devices::shelly::shelly::ShellyClient,
    terminator::{TestamentReceiver, TestamentSender},
};

mod ble;
mod cloud;
mod eth;
mod input;
mod light;
mod mqtt;
mod shelly;
mod switch;

pub fn activate_devices(
    shelly: &Shelly,
    event_registry: &EventRegistry,
    running_connections: &mut Vec<TestamentSender>,
    devices: ShellyDevices,
    status_updater: mpsc::Sender<StateUpdateMessage>,
) {
    let shelly_devices = Arc::new(devices);
    if shelly.endpoints().is_empty() {
        do_activate_devices(
            shelly_devices.endpoints.clone().iter().cloned(),
            event_registry,
            running_connections,
            shelly_devices,
            status_updater,
        );
    } else {
        do_activate_devices(
            shelly.endpoints().iter().map(|ep| ep.address()),
            event_registry,
            running_connections,
            shelly_devices,
            status_updater,
        );
    };
}

fn do_activate_devices(
    devices: impl Iterator<Item = IpAddr>,
    event_registry: &EventRegistry,
    running_connections: &mut Vec<TestamentSender>,
    shelly_devices: Arc<ShellyDevices>,
    status_updater: mpsc::Sender<StateUpdateMessage>,
) {
    info!("Shelly device changed");
    running_connections.clear();
    for ep in devices {
        running_connections.push(start_enumeration_listener(
            ep,
            event_registry.clone(),
            shelly_devices.clone(),
            status_updater.clone(),
        ));
    }
}
fn start_enumeration_listener(
    connection: IpAddr,
    event_registry: EventRegistry,
    shelly_devices: Arc<ShellyDevices>,
    status_updater: mpsc::Sender<StateUpdateMessage>,
) -> TestamentSender {
    let (testament, testament_stream) = TestamentSender::create();
    task::spawn(async move {
        let socket_str = format!("{connection:?}");
        loop {
            info!("Connect to {socket_str}");
            match run_enumeration_listener(
                connection,
                event_registry.clone(),
                shelly_devices.clone(),
                testament_stream.clone(),
                status_updater.clone(),
            )
            .await
            {
                Ok(_) => {
                    info!("{socket_str}: Finished");
                    break;
                }
                Err(e) => {
                    error!("{socket_str}: Error: {e}");
                    if let ShellyError::ClientError(client::Error::ParseError(e)) = e {
                        error!(
                            "{socket_str}: Parse Error at {}:{}: {e}",
                            e.line(),
                            e.column()
                        );
                    }
                    if let Err(error) = status_updater
                        .send(StateUpdateMessage::EndpointDisconnected(connection))
                        .await
                    {
                        error!(
                            "Cannot send status update on connection to {}: {error}",
                            connection
                        );
                    }
                }
            };
            sleep(Duration::from_secs(10)).await;
        }
        status_updater
            .send(StateUpdateMessage::EndpointDisconnected(connection))
            .await
            .expect("Cannot send status update");
    });
    testament
}

async fn run_enumeration_listener(
    addr: IpAddr,
    event_registry: EventRegistry,
    shelly_devices: Arc<ShellyDevices>,
    termination: TestamentReceiver,
    status_updater: mpsc::Sender<StateUpdateMessage>,
) -> Result<(), ShellyError> {
    let uri = Url::parse(&format!("ws://{}/rpc", addr))?;

    let (tx, rx) = WsTransportClientBuilder::default().build(uri).await?;
    let client: Client = ClientBuilder::default().build_with_tokio(tx, rx);
    let result = client.get_deviceinfo(false).await?;
    let mut offset = 0;
    let mut component_entries = Vec::new();
    loop {
        let response = client.get_components(offset, false).await?;
        for entry in response.components().iter().cloned() {
            component_entries.push(entry);
        }
        if response.total() as usize >= component_entries.len() {
            break;
        }
        offset += response.components().len() as u16;
    }
    info!("Found Components: {component_entries:#?}");
    Ok(())
}

#[derive(Error, Debug)]
enum ShellyError {
    #[error("Connection to endpoint lost")]
    ConnectionLost,
    #[error("Invalid address: {0}")]
    InvalidAddress(#[from] url::ParseError),
    #[error("Websocket Handshake Error: {0}")]
    WsHandshakeError(#[from] WsHandshakeError),
    #[error("JSON RPC Error: {0}")]
    ClientError(#[from] ClientError),
}
