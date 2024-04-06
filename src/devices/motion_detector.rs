use std::time::Duration;

use log::{debug, error};
use thiserror::Error;
use tinkerforge_async::{
    base58::Base58Error, error::TinkerforgeError, motion_detector_v_2::MotionDetectorV2Bricklet,
};
use tokio::{sync::mpsc, task::JoinHandle, time::sleep};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};

use crate::terminator::LifeLineEnd;
use crate::{
    data::{
        registry::{ButtonState, EventRegistry, SingleButtonKey, SingleButtonLayout},
        state::StateUpdateMessage,
    },
    terminator::{TestamentReceiver, TestamentSender},
};

pub fn handle_motion_detector(
    bricklet: MotionDetectorV2Bricklet,
    event_registry: EventRegistry,
    single_button_key: SingleButtonKey,
) -> LifeLineEnd {
    let (foreign_end, my_end) = LifeLineEnd::create();
    tokio::spawn(async move {
        if let Err(error) =
            motion_detector_task(bricklet, event_registry, single_button_key, my_end).await
        {
            error!("Error processing motion detection: {error}");
        }
    });
    foreign_end
}

#[derive(Error, Debug)]
enum MotionDetectorError {
    #[error("Tinkerforge error: {0}")]
    Tinkerforge(#[from] TinkerforgeError),
    #[error("Send error: {0}")]
    SendError(#[from] mpsc::error::SendError<ButtonState<SingleButtonLayout>>),
    #[error("Cannot update state: {0}")]
    StateUpdate(#[from] mpsc::error::SendError<StateUpdateMessage>),
    #[error("Cannot parse UID {0}")]
    Uid(#[from] Base58Error),
}

#[derive(Debug)]
enum MotionDetectionEvent {
    MotionStarted,
    MotionEnded,
    Closed,
}

async fn motion_detector_task(
    mut bricklet: MotionDetectorV2Bricklet,
    event_registry: EventRegistry,
    single_button_key: SingleButtonKey,
    termination_receiver: LifeLineEnd,
) -> Result<(), MotionDetectorError> {
    bricklet.set_sensitivity(100).await?;
    let (tx, rx) = mpsc::channel(2);

    let sender = event_registry.single_button_sender(single_button_key).await;

    let mut stream = bricklet
        .motion_detected_stream()
        .await
        .map(|_| MotionDetectionEvent::MotionStarted)
        .merge(termination_receiver.send_on_terminate(MotionDetectionEvent::Closed))
        .merge(ReceiverStream::new(rx));
    let mut timer_handle = None::<JoinHandle<()>>;

    while let Some(event) = stream.next().await {
        debug!("Motion event: {event:?}");
        match event {
            MotionDetectionEvent::MotionStarted => {
                sender
                    .send(ButtonState::ShortPressStart(SingleButtonLayout {}))
                    .await?;
                let tx = tx.clone();
                if let Some(handle) = timer_handle.replace(tokio::spawn(async move {
                    sleep(Duration::from_millis(500)).await;
                    if let Err(error) = tx.send(MotionDetectionEvent::MotionEnded).await {
                        error!("Error sending motion detector events: {error}");
                    }
                })) {
                    handle.abort();
                }
            }
            MotionDetectionEvent::MotionEnded => {
                sender.send(ButtonState::Released).await?;
            }
            MotionDetectionEvent::Closed => break,
        }
    }
    drop(termination_receiver);
    Ok(())
}
