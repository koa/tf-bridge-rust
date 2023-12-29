use std::time::SystemTime;

use log::{error, info};
use thiserror::Error;
use tinkerforge_async::{error::TinkerforgeError, io16_v2_bricklet::Io16V2Bricklet};
use tokio::sync::mpsc;
use tokio_stream::{wrappers::ReceiverStream, StreamExt};

use crate::registry::{ButtonState, DualButtonKey, DualButtonLayout, EventRegistry};

pub async fn handle_io16_v2(
    bricklet: Io16V2Bricklet,
    event_registry: EventRegistry,
    dual_buttons: &[DualButtonSettings],
) -> mpsc::Sender<()> {
    let (tx, rx) = mpsc::channel(1);
    let mut button_settings = <[ButtonSetting; 16]>::default();
    for setting in dual_buttons {
        if let Some(b) = button_settings.get_mut(setting.up_button as usize) {
            *b = ButtonSetting::DualButtonUp(
                event_registry.dual_button_sender(setting.output).await,
            );
        } else {
            error!("On Button out of range: {}", setting.up_button);
        }
        if let Some(b) = button_settings.get_mut(setting.down_button as usize) {
            *b = ButtonSetting::DualButtonDown(
                event_registry.dual_button_sender(setting.output).await,
            );
        } else {
            error!("Off Button out of range: {}", setting.up_button);
        }
    }
    tokio::spawn(async move {
        match io_16_v2_loop(bricklet, rx, button_settings).await {
            Err(error) => {
                error!("Cannot communicate with Io16V2Bricklet: {error}");
            }
            Ok(_) => {
                info!("Io16V2Bricklet done");
            }
        }
    });
    tx
}

pub struct DualButtonSettings {
    pub up_button: u8,
    pub down_button: u8,
    pub output: DualButtonKey,
}
#[derive(Debug)]
enum IoMessage {
    Close,
    Press(u8),
    Release(u8),
    Noop,
}
#[derive(Clone, Default)]
enum ButtonSetting {
    #[default]
    None,
    DualButtonUp(mpsc::Sender<ButtonState<DualButtonLayout>>),
    DualButtonDown(mpsc::Sender<ButtonState<DualButtonLayout>>),
}

async fn io_16_v2_loop(
    mut bricklet: Io16V2Bricklet,
    termination_receiver: mpsc::Receiver<()>,
    button_settings: [ButtonSetting; 16],
) -> Result<(), IoHandlerError> {
    let uid = bricklet
        .get_identity()
        .await
        .ok()
        .map(|i| i.uid)
        .unwrap_or_default();
    let button_event_stream = bricklet
        .get_input_value_callback_receiver()
        .await
        .map(|event| {
            if !event.value {
                IoMessage::Press(event.channel)
            } else {
                IoMessage::Release(event.channel)
            }
        });
    let mut receiver = button_event_stream
        .merge(ReceiverStream::new(termination_receiver).map(|_| IoMessage::Close));
    while let Some(message) = receiver.next().await {
        match message {
            IoMessage::Close => break,
            IoMessage::Press(channel) => match button_settings.get(channel as usize) {
                None => {}
                Some(ButtonSetting::DualButtonDown(sender)) => sender
                    .send(ButtonState::ShortPressStart(DualButtonLayout::DOWN))
                    .await
                    .map_err(IoHandlerError::DualButtonDown)?,
                Some(ButtonSetting::DualButtonUp(sender)) => sender
                    .send(ButtonState::ShortPressStart(DualButtonLayout::UP))
                    .await
                    .map_err(IoHandlerError::DualButtonUp)?,
                Some(ButtonSetting::None) => {}
            },
            IoMessage::Release(channel) => match button_settings.get(channel as usize) {
                None => {}
                Some(ButtonSetting::DualButtonDown(sender)) => sender
                    .send(ButtonState::Released)
                    .await
                    .map_err(IoHandlerError::DualButtonRelease)?,
                Some(ButtonSetting::DualButtonUp(sender)) => sender
                    .send(ButtonState::Released)
                    .await
                    .map_err(IoHandlerError::DualButtonRelease)?,
                Some(ButtonSetting::None) => {}
            },
            IoMessage::Noop => {}
        };
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum IoHandlerError {
    #[error("Cannot communicate to device: {0}")]
    Communication(#[from] TinkerforgeError),
    #[error("Cannot send Dual button Down: {0}")]
    DualButtonDown(mpsc::error::SendError<ButtonState<DualButtonLayout>>),
    #[error("Cannot send Dual button Up: {0}")]
    DualButtonUp(mpsc::error::SendError<ButtonState<DualButtonLayout>>),
    #[error("Cannot send Dual button Release: {0}")]
    DualButtonRelease(mpsc::error::SendError<ButtonState<DualButtonLayout>>),
}
