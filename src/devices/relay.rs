use futures::{Stream, stream::SelectAll};
use log::error;
use thiserror::Error;
use tinkerforge_async::{
    base58::Base58Error, error::TinkerforgeError,
    industrial_quad_relay_v_2::IndustrialQuadRelayV2Bricklet,
};
use tokio::{
    sync::mpsc::{self, Sender},
    task::JoinHandle,
    time::sleep,
};
use tokio_stream::{StreamExt, wrappers::ReceiverStream};

use crate::{
    data::{registry::EventRegistry, state::StateUpdateMessage, wiring::RelayChannelEntry},
    terminator::LifeLineEnd,
};

pub async fn handle_quad_relay(
    bricklet: IndustrialQuadRelayV2Bricklet,
    event_registry: &EventRegistry,
    inputs: &[RelayChannelEntry],
) -> LifeLineEnd {
    let (tx, rx) = LifeLineEnd::create();
    let mut streams = SelectAll::new();
    for channel_entry in inputs {
        let channel = channel_entry.channel;
        streams.push(
            event_registry
                .switch_stream(channel_entry.input)
                .await
                .map(move |state| RelayMsg::SetState(channel, state)),
        );
    }
    let input_streams = streams.merge(rx.send_on_terminate(RelayMsg::Closed));
    tokio::spawn(async move {
        if let Err(error) = quad_relay_task(bricklet, input_streams).await {
            error!("Error processing relay: {error}");
        }
        drop(rx);
    });
    tx
}

enum RelayMsg {
    SetState(u8, bool),
    UpdateState,
    Closed,
}

#[derive(Debug, Error)]
enum RelayError {
    #[error("Tinkerforge error: {0}")]
    Tinkerforge(#[from] TinkerforgeError),
    #[error("Cannot send status update {0}")]
    StatusUpdate(#[from] mpsc::error::SendError<StateUpdateMessage>),
    #[error("Cannot parse UID {0}")]
    Uid(#[from] Base58Error),
}

async fn quad_relay_task(
    mut bricklet: IndustrialQuadRelayV2Bricklet,
    input_stream: impl Stream<Item=RelayMsg> + Unpin,
) -> Result<(), RelayError> {
    let (tx, rx) = mpsc::channel(2);
    let mut stream = input_stream.merge(ReceiverStream::new(rx));
    let mut current_value = [false; 4];
    let mut timer_handle = Some(start_send_timer(&tx));
    while let Some(event) = stream.next().await {
        match event {
            RelayMsg::SetState(channel, state) => {
                let mut new_value = current_value;
                new_value[channel as usize] = state;
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
