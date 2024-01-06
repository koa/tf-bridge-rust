use core::option::Option;

use log::{error, info};
use thiserror::Error;
use tinkerforge_async::{error::TinkerforgeError, io16_v2_bricklet::Io16V2Bricklet};
use tokio::{sync::mpsc, task::JoinHandle, time::sleep};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};

use crate::data::registry::{ButtonState, DualButtonLayout, EventRegistry, SingleButtonLayout};
use crate::data::wiring::ButtonSetting;

pub async fn handle_io16_v2(
    bricklet: Io16V2Bricklet,
    event_registry: EventRegistry,
    buttons: &[ButtonSetting],
) -> mpsc::Sender<()> {
    let (tx, rx) = mpsc::channel(1);
    let mut channel_settings = <[ChannelSetting; 16]>::default();
    for setting in buttons {
        match setting {
            ButtonSetting::Dual {
                up_button,
                down_button,
                output,
            } => {
                if let Some(b) = channel_settings.get_mut(*up_button as usize) {
                    *b = ChannelSetting::DualButtonUp(
                        event_registry.dual_button_sender(*output).await,
                    );
                } else {
                    error!("On Button out of range: {}", up_button);
                }
                if let Some(b) = channel_settings.get_mut(*down_button as usize) {
                    *b = ChannelSetting::DualButtonDown(
                        event_registry.dual_button_sender(*output).await,
                    );
                } else {
                    error!("Off Button out of range: {}", up_button);
                }
            }
            ButtonSetting::Single { button, output } => {
                if let Some(b) = channel_settings.get_mut(*button as usize) {
                    *b = ChannelSetting::SingleButton(
                        event_registry.single_button_sender(*output).await,
                    );
                } else {
                    error!("Button out of range: {}", button);
                }
            }
        }
    }
    tokio::spawn(async move {
        match io_16_v2_loop(bricklet, rx, channel_settings).await {
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

#[derive(Debug)]
enum IoMessage {
    Close,
    Press(u8),
    LongPress(u8),
    Release(u8),
}
#[derive(Clone, Default)]
enum ChannelSetting {
    #[default]
    None,
    DualButtonUp(mpsc::Sender<ButtonState<DualButtonLayout>>),
    DualButtonDown(mpsc::Sender<ButtonState<DualButtonLayout>>),
    SingleButton(mpsc::Sender<ButtonState<SingleButtonLayout>>),
}

async fn io_16_v2_loop(
    mut bricklet: Io16V2Bricklet,
    termination_receiver: mpsc::Receiver<()>,
    channel_settings: [ChannelSetting; 16],
) -> Result<(), IoHandlerError> {
    let (rx, tx) = mpsc::channel(2);
    let mut channel_timer: [Option<JoinHandle<()>>; 16] = <[Option<JoinHandle<()>>; 16]>::default();

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
        .merge(ReceiverStream::new(termination_receiver).map(|_| IoMessage::Close))
        .merge(ReceiverStream::new(tx));
    while let Some(message) = receiver.next().await {
        match message {
            IoMessage::Close => break,
            IoMessage::Press(channel) => {
                match channel_settings.get(channel as usize) {
                    None => {}
                    Some(ChannelSetting::DualButtonDown(sender)) => sender
                        .send(ButtonState::ShortPressStart(DualButtonLayout::Down))
                        .await
                        .map_err(IoHandlerError::DualButtonDown)?,
                    Some(ChannelSetting::DualButtonUp(sender)) => sender
                        .send(ButtonState::ShortPressStart(DualButtonLayout::Up))
                        .await
                        .map_err(IoHandlerError::DualButtonUp)?,
                    Some(ChannelSetting::None) => {}
                    Some(ChannelSetting::SingleButton(sender)) => sender
                        .send(ButtonState::ShortPressStart(SingleButtonLayout))
                        .await
                        .map_err(IoHandlerError::SingleButton)?,
                }

                let rx = rx.clone();
                if let Some(running) =
                    channel_timer
                        .get_mut(channel as usize)
                        .and_then(|timer_option| {
                            timer_option.replace(tokio::spawn(async move {
                                sleep(core::time::Duration::from_millis(500)).await;
                                if let Err(error) = rx.send(IoMessage::LongPress(channel)).await {
                                    error!("Cannot send message: {error}");
                                }
                            }))
                        })
                {
                    running.abort();
                }
            }
            IoMessage::LongPress(channel) => {
                match channel_settings.get(channel as usize) {
                    None => {}
                    Some(ChannelSetting::DualButtonDown(sender)) => sender
                        .send(ButtonState::LongPressStart(DualButtonLayout::Down))
                        .await
                        .map_err(IoHandlerError::DualButtonDown)?,
                    Some(ChannelSetting::DualButtonUp(sender)) => sender
                        .send(ButtonState::LongPressStart(DualButtonLayout::Up))
                        .await
                        .map_err(IoHandlerError::DualButtonUp)?,
                    Some(ChannelSetting::None) => {}
                    Some(ChannelSetting::SingleButton(sender)) => sender
                        .send(ButtonState::LongPressStart(SingleButtonLayout))
                        .await
                        .map_err(IoHandlerError::SingleButton)?,
                }
                let rx = rx.clone();
                if let Some(running) =
                    channel_timer
                        .get_mut(channel as usize)
                        .and_then(|timer_option| {
                            timer_option.replace(tokio::spawn(async move {
                                sleep(core::time::Duration::from_secs(20)).await;
                                if let Err(error) = rx.send(IoMessage::Release(channel)).await {
                                    error!("Cannot send message: {error}");
                                }
                            }))
                        })
                {
                    running.abort();
                }
            }
            IoMessage::Release(channel) => {
                match channel_settings.get(channel as usize) {
                    None => {}
                    Some(ChannelSetting::DualButtonDown(sender)) => sender
                        .send(ButtonState::Released)
                        .await
                        .map_err(IoHandlerError::DualButtonRelease)?,
                    Some(ChannelSetting::DualButtonUp(sender)) => sender
                        .send(ButtonState::Released)
                        .await
                        .map_err(IoHandlerError::DualButtonRelease)?,
                    Some(ChannelSetting::None) => {}
                    Some(ChannelSetting::SingleButton(sender)) => sender
                        .send(ButtonState::Released)
                        .await
                        .map_err(IoHandlerError::SingleButtonRelease)?,
                }
                if let Some(timer) = channel_timer
                    .get_mut(channel as usize)
                    .and_then(|t| t.take())
                {
                    timer.abort();
                }
            }
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
    #[error("Cannot send Single button press: {0}")]
    SingleButton(mpsc::error::SendError<ButtonState<SingleButtonLayout>>),
    #[error("Cannot send Dual button Release: {0}")]
    DualButtonRelease(mpsc::error::SendError<ButtonState<DualButtonLayout>>),
    #[error("Cannot send Single button Release: {0}")]
    SingleButtonRelease(mpsc::error::SendError<ButtonState<SingleButtonLayout>>),
}
