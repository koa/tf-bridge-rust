use crate::devices::shelly::shelly::ComponentKey;
use crate::devices::HandleRegistry;
use crate::{
    data::{
        registry::EventRegistry, settings::Shelly, state::StateUpdateMessage, wiring::ShellyDevices,
    },
    devices::shelly::shelly::{ComponentEntry, ShellyClient},
    terminator::{TestamentReceiver, TestamentSender},
};
use jsonrpsee::{
    client_transport::{
        ws::WsHandshakeError,
        ws::{Url, WsTransportClientBuilder},
    },
    core::{
        client::{self, Client, ClientBuilder, Error},
        ClientError,
    },
};
use log::{error, info};
use std::ops::Deref;
use std::{
    fmt::{Display, Formatter},
    net::IpAddr,
    sync::Arc,
    time::Duration,
};
use thiserror::Error;
use tokio::sync::Mutex;
use tokio::{sync::mpsc, task, time::sleep};

pub mod ble;
pub mod bthome;
pub mod cloud;
pub mod common;
pub mod eth;
pub mod input;
pub mod knx;
pub mod light;
pub mod mqtt;
pub mod shelly;
pub mod switch;
pub mod sys;
pub mod ui;
pub mod wifi;
pub mod ws;

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
        let mut running_components = HandleRegistry::default();

        loop {
            info!("Connect to {socket_str}");
            let result = run_enumeration_listener(
                connection,
                event_registry.clone(),
                shelly_devices.clone(),
                testament_stream.clone(),
                status_updater.clone(),
                &mut running_components,
            )
            .await;
            info!("Connected to {socket_str}: {result:?}");
            match result {
                Ok(_) => {
                    info!("{socket_str}: Finished");
                    break;
                }
                Err(ShellyEndpointError {
                    base_error,
                    endpoint,
                }) => {
                    error!("{socket_str}: Error: {base_error}");
                    if let ShellyError::ClientError(client::Error::ParseError(e)) = base_error {
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
    running_components: &mut HandleRegistry<ComponentKey>,
) -> Result<(), ShellyEndpointError> {
    let uri = Url::parse(&format!("ws://{}/rpc", addr)).map_err(enrich_error(addr))?;

    let (tx, rx) = WsTransportClientBuilder::default()
        .build(uri)
        .await
        .map_err(enrich_error(addr))?;
    let client: Client = ClientBuilder::default().build_with_tokio(tx, rx);
    let client = Arc::new(Mutex::new(client));
    let result = client
        .lock()
        .await
        .get_deviceinfo(false)
        .await
        .map_err(enrich_error(addr))?;

    status_updater
        .send(StateUpdateMessage::EndpointConnected {
            address: addr,
            hostname: Some(result.id.to_string().into_boxed_str()),
        })
        .await
        .map_err(enrich_error(addr))?;
    let mut offset = 0;
    let mut component_entries = Vec::new();
    loop {
        //info!("{addr} fetch from offset {offset}");
        let component_batch_result = client.lock().await.get_components(offset, false).await;
        //info!("{addr}: comps: {result1:#?}");
        match component_batch_result {
            Ok(response) => {
                for entry in response.components().iter().cloned() {
                    component_entries.push(entry);
                }
                /*info!(
                    "{addr} total: {}, received: {}",
                    response.total(),
                    response.components().len()
                );*/
                if response.total() as usize <= component_entries.len() {
                    break;
                }
                offset += response.components().len() as u16;
            }
            Err(Error::ParseError(e)) => {
                let string = client
                    .lock()
                    .await
                    .get_components_string(offset, false)
                    .await
                    .map_err(enrich_error(addr))?;
                error!("Cannot parse response from {string} from {addr}: {e}");
                return Ok(());
            }
            Err(e) => {
                error!("Unexpected error from {addr}: {e}");
                return Err(ShellyEndpointError {
                    base_error: e.into(),
                    endpoint: addr,
                });
            }
        }
    }
    info!("{addr} Found Components: {}", component_entries.len());
    let configured_device = shelly_devices.devices.get(&result.id);

    for entry in component_entries {
        status_updater
            .send(StateUpdateMessage::ShellyComponentFound {
                id: result.id,
                endpoint: addr,
                component: entry.clone(),
            })
            .await
            .map_err(enrich_error(addr))?;
        match entry {
            ComponentEntry::Input(_) => {}
            ComponentEntry::Ble(_) => {}
            ComponentEntry::Cloud(_) => {}
            ComponentEntry::Eth(_) => {}
            ComponentEntry::Light(light) => {
                info!("Light: {}", light.key.id);
                let id = ComponentKey::Light(light.key);
                let found_light_config = configured_device.and_then(|d| d.lights.get(&light.key));
                if let Some(light_config) = found_light_config {
                    let handle = light.run(&client, light_config, event_registry.clone());
                    running_components.insert(id, handle);
                } else {
                    running_components.remove(&id);
                }
            }
            ComponentEntry::Mqtt(_) => {}
            ComponentEntry::Switch(switch) => {
                info!("Switch: {}", switch.key.id);

                switch
                    .set(
                        client.lock().await.deref(),
                        true,
                        Some(chrono::Duration::seconds(2)),
                    )
                    .await
                    .map_err(enrich_error(addr))?;
            }
            ComponentEntry::Sys(_) => {}
            ComponentEntry::Wifi(mut wifi) => {
                wifi.disable(client.lock().await.deref())
                    .await
                    .map_err(enrich_error(addr))?;
            }
            ComponentEntry::Ui(_) => {}
            ComponentEntry::Ws(_) => {}
            ComponentEntry::Bthome(_) => {}
            ComponentEntry::Knx(_) => {}
        }
    }
    while client.lock().await.is_connected() {
        sleep(Duration::from_secs(10)).await;
    }
    Ok(())
}

fn enrich_error<E: Into<ShellyError> + Sized>(
    addr: IpAddr,
) -> impl Fn(E) -> ShellyEndpointError + Sized {
    move |error| ShellyEndpointError {
        base_error: error.into(),
        endpoint: addr,
    }
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
    #[error("Cannot update status message {0}")]
    StatusUpdateMessage(#[from] mpsc::error::SendError<StateUpdateMessage>),
}

#[derive(Error, Debug)]
struct ShellyEndpointError {
    base_error: ShellyError,
    endpoint: IpAddr,
}

impl Display for ShellyEndpointError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Error: {} at shelly {}", self.base_error, self.endpoint)
    }
}
