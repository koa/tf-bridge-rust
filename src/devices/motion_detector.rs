use std::time::Duration;

use log::error;
use thiserror::Error;
use tinkerforge_async::{
    error::TinkerforgeError, motion_detector_v2_bricklet::MotionDetectorV2Bricklet,
};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::sleep;
use tokio_stream::{wrappers::ReceiverStream, StreamExt};

use crate::data::registry::{ButtonState, EventRegistry, SingleButtonKey, SingleButtonLayout};
use crate::terminator::{TestamentReceiver, TestamentSender};

pub fn handle_motion_detector(
    bricklet: MotionDetectorV2Bricklet,
    event_registry: EventRegistry,
    single_button_key: SingleButtonKey,
) -> TestamentSender {
    let (tx, rx) = TestamentSender::create();
    tokio::spawn(async move {
        if let Err(error) =
            motion_detector_task(bricklet, event_registry, single_button_key, rx).await
        {
            error!("Error processing motion detection: {error}");
        }
    });
    tx
}
#[derive(Error, Debug)]
enum MotionDetectorError {
    #[error("Tinkerforge error: {0}")]
    Tinkerforge(#[from] TinkerforgeError),
    #[error("Send error: {0}")]
    SendError(#[from] mpsc::error::SendError<ButtonState<SingleButtonLayout>>),
}
enum MotionDetectionEvent {
    MotionStarted,
    MotionEnded,
    Closed,
}
async fn motion_detector_task(
    mut bricklet: MotionDetectorV2Bricklet,
    event_registry: EventRegistry,
    single_button_key: SingleButtonKey,
    termination_receiver: TestamentReceiver,
) -> Result<(), MotionDetectorError> {
    bricklet.set_sensitivity(100).await?;
    let (tx, rx) = mpsc::channel(2);

    let sender = event_registry.single_button_sender(single_button_key).await;
    let mut stream = bricklet
        .get_motion_detected_callback_receiver()
        .await
        .map(|_| MotionDetectionEvent::MotionStarted)
        .merge(termination_receiver.send_on_terminate(MotionDetectionEvent::Closed))
        .merge(ReceiverStream::new(rx));
    let mut timer_handle = None::<JoinHandle<()>>;

    while let Some(event) = stream.next().await {
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
    Ok(())
}
