use log::error;
use thiserror::Error;
use tinkerforge_async::error::TinkerforgeError;
use tinkerforge_async::motion_detector_v2_bricklet::MotionDetectorV2Bricklet;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use crate::registry::{ButtonState, EventRegistry, SingleButtonKey, SingleButtonLayout};

pub fn handle_motion_detector(
    bricklet: MotionDetectorV2Bricklet,
    event_registry: EventRegistry,
    single_button_key: SingleButtonKey,
) -> mpsc::Sender<()> {
    let (tx, rx) = mpsc::channel(1);
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
    MotionDetected,
    Closed,
}
async fn motion_detector_task(
    mut bricklet: MotionDetectorV2Bricklet,
    event_registry: EventRegistry,
    single_button_key: SingleButtonKey,
    rx: mpsc::Receiver<()>,
) -> Result<(), MotionDetectorError> {
    bricklet.set_sensitivity(100).await?;
    let sender = event_registry.single_button_sender(single_button_key).await;
    let mut stream = bricklet
        .get_motion_detected_callback_receiver()
        .await
        .map(|_| MotionDetectionEvent::MotionDetected)
        .merge(ReceiverStream::new(rx).map(|_| MotionDetectionEvent::Closed));
    while let Some(event) = stream.next().await {
        match event {
            MotionDetectionEvent::MotionDetected => {
                sender
                    .send(ButtonState::ShortPressStart(SingleButtonLayout {}))
                    .await?;
                sender.send(ButtonState::Released).await?;
            }
            MotionDetectionEvent::Closed => break,
        }
    }
    Ok(())
}
