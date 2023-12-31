use futures::{SinkExt, Stream};
use log::error;
use thiserror::Error;
use tinkerforge_async::error::TinkerforgeError;
use tinkerforge_async::industrial_quad_relay_bricklet::IndustrialQuadRelayBricklet;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use crate::data::registry::{EventRegistry, SwitchOutputKey};
use crate::util::optional_stream;

pub fn handle_quad_releay(
    bricklet: IndustrialQuadRelayBricklet,
    event_registry: EventRegistry,
    inputs: [Option<SwitchOutputKey>; 4],
) -> Sender<()> {
    let (tx, rx) = mpsc::channel(1);
    tokio::spawn(async move {
        if let Err(error) = quad_relay_task(bricklet, event_registry, inputs, rx).await {
            error!("Error processing temperature: {error}");
        }
    });
    tx
}
#[derive(Error, Debug)]
enum RelayError {
    #[error("Tinkerforge error: {0}")]
    Tinkerforge(#[from] TinkerforgeError),
    #[error("Send error: {0}")]
    SendError(#[from] mpsc::error::SendError<f32>),
}
enum RelayMsg {
    SetState(u8, bool),
    UpdateState,
    Closed,
}
async fn quad_relay_task(
    mut bricklet: IndustrialQuadRelayBricklet,
    event_registry: EventRegistry,
    inputs: [Option<SwitchOutputKey>; 4],
    termination_receiver: mpsc::Receiver<()>,
) -> Result<(), RelayError> {
    let (tx, rx) = mpsc::channel(2);

    let mut stream = relay_stream(&event_registry, inputs, 0)
        .await
        .merge(relay_stream(&event_registry, inputs, 1).await)
        .merge(relay_stream(&event_registry, inputs, 2).await)
        .merge(relay_stream(&event_registry, inputs, 3).await)
        .merge(ReceiverStream::new(termination_receiver).map(|_| RelayMsg::Closed))
        .merge(ReceiverStream::new(rx));
    let mut current_value = 0;
    let mut timer_handle = Some(start_send_timer(&tx));
    bricklet.set_monoflop(0x0f, 0, 0).await?;

    while let Some(event) = stream.next().await {
        match event {
            RelayMsg::SetState(channel, state) => {
                let new_value = if state {
                    current_value | 1 << channel
                } else {
                    current_value & !1 << channel
                };
                if new_value != current_value {
                    current_value = new_value;
                    if let Some(old_timer) = timer_handle.replace(start_send_timer(&tx)) {
                        old_timer.abort();
                    }
                }
            }
            RelayMsg::Closed => {}
            RelayMsg::UpdateState => {
                bricklet.set_value(current_value).await?;
            }
        }
    }
    Ok(())
}

fn start_send_timer(tx: &Sender<RelayMsg>) -> JoinHandle<()> {
    let tx = tx.clone();
    tokio::spawn(async move {
        sleep(core::time::Duration::from_millis(10)).await;
        tx.send(RelayMsg::UpdateState)
            .await
            .expect("Cannot enqueue update message");
    })
}

async fn relay_stream(
    event_registry: &EventRegistry,
    inputs: [Option<SwitchOutputKey>; 4],
    idx: u8,
) -> impl Stream<Item = RelayMsg> + Sized {
    optional_stream(inputs[idx as usize].map(|key| event_registry.switch_stream(key)))
        .await
        .map(move |state| RelayMsg::SetState(idx, state))
}
