use core::option::Option;
use std::net::IpAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Stream;
use log::{error, info};
use thiserror::Error;
use tinkerforge_async::base58::Base58Error;
use tinkerforge_async::io16_v2_bricklet::IO16_V2_BRICKLET_STATUS_LED_CONFIG_OFF;
use tinkerforge_async::{
    error::TinkerforgeError,
    io16_bricklet::{InterruptEvent, Io16Bricklet},
    io16_v2_bricklet::Io16V2Bricklet,
};
use tokio::{sync::mpsc, task::JoinHandle, time::sleep};
use tokio_stream::{empty, wrappers::ReceiverStream, StreamExt};
use tokio_util::either::Either;

use crate::data::state::StateUpdateMessage;
use crate::data::{
    registry::{ButtonState, DualButtonLayout, EventRegistry, SingleButtonLayout},
    wiring::ButtonSetting,
};
use crate::terminator::{TestamentReceiver, TestamentSender};

pub async fn handle_io16(
    bricklet: Io16Bricklet,
    event_registry: EventRegistry,
    buttons: &[ButtonSetting],
    status_updater: mpsc::Sender<StateUpdateMessage>,
    ip_addr: IpAddr,
) -> TestamentSender {
    let (tx, rx) = TestamentSender::create();
    let channel_settings = collect_16_channel_settings(event_registry, buttons).await;
    tokio::spawn(async move {
        let result = io_16_v1_loop(bricklet, rx, channel_settings, status_updater, ip_addr).await;
        match result {
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

async fn collect_16_channel_settings(
    event_registry: EventRegistry,
    buttons: &[ButtonSetting],
) -> [ChannelSetting; 16] {
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
    channel_settings
}

struct ByteMaskIterator {
    value: u8,
    mask: u8,
    index: u8,
}

impl Stream for ByteMaskIterator {
    type Item = IoMessage;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(Iterator::next(self.get_mut()))
    }
}

impl Iterator for ByteMaskIterator {
    type Item = IoMessage;

    fn next(&mut self) -> Option<Self::Item> {
        while self.mask != 0 && self.mask & 1 == 0 {
            self.index += 1;
            self.mask >>= 1;
            self.value >>= 1;
        }
        if self.mask == 0 {
            None
        } else {
            let event = Some(if self.value & 1 == 1 {
                IoMessage::Release(self.index)
            } else {
                IoMessage::Press(self.index)
            });
            self.index += 1;
            self.mask >>= 1;
            self.value >>= 1;
            event
        }
    }
}

async fn io_16_v1_loop(
    mut bricklet: Io16Bricklet,
    rx: TestamentReceiver,
    channel_settings: [ChannelSetting; 16],
    status_updater: mpsc::Sender<StateUpdateMessage>,
    ip_addr: IpAddr,
) -> Result<(), IoHandlerError> {
    bricklet.set_debounce_period(30).await?;
    bricklet.set_port_interrupt('a', 0xff).await?;
    bricklet.set_port_interrupt('b', 0xff).await?;
    let id = bricklet.get_identity().await?;
    let uid = id.uid.parse()?;
    status_updater.send((ip_addr, id).try_into()?).await?;
    let button_event_stream = futures::StreamExt::flat_map(
        bricklet.get_interrupt_callback_receiver().await,
        move |InterruptEvent {
                  port,
                  interrupt_mask,
                  value_mask,
              }| {
            if let Some(start_idx) = match port {
                'a' => Some(0),
                'b' => Some(8),
                _ => None,
            } {
                Either::Left(ByteMaskIterator {
                    value: value_mask,
                    mask: interrupt_mask,
                    index: start_idx,
                })
            } else {
                Either::Right(empty())
            }
        },
    );
    io_16_loop(
        rx,
        channel_settings,
        button_event_stream,
        status_updater.clone(),
    )
    .await?;
    status_updater
        .send(StateUpdateMessage::BrickletDisconnected {
            uid,
            endpoint: ip_addr,
        })
        .await?;
    Ok(())
}

pub async fn handle_io16_v2(
    bricklet: Io16V2Bricklet,
    event_registry: EventRegistry,
    buttons: &[ButtonSetting],
    status_updater: mpsc::Sender<StateUpdateMessage>,
    ip_addr: IpAddr,
) -> TestamentSender {
    let (tx, rx) = TestamentSender::create();
    let channel_settings = collect_16_channel_settings(event_registry, buttons).await;
    tokio::spawn(async move {
        match io_16_v2_loop(bricklet, rx, channel_settings, status_updater, ip_addr).await {
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

#[derive(Debug, PartialEq)]
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
    termination_receiver: TestamentReceiver,
    channel_settings: [ChannelSetting; 16],
    status_updater: mpsc::Sender<StateUpdateMessage>,
    ip_addr: IpAddr,
) -> Result<(), IoHandlerError> {
    for i in 0..16 {
        bricklet
            .set_input_value_callback_configuration(i, 20, true)
            .await?;
    }
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
    bricklet
        .set_status_led_config(IO16_V2_BRICKLET_STATUS_LED_CONFIG_OFF)
        .await?;
    let identity = bricklet.get_identity().await?;
    let uid = identity.uid.parse()?;
    status_updater.send((ip_addr, identity).try_into()?).await?;
    io_16_loop(
        termination_receiver,
        channel_settings,
        button_event_stream,
        status_updater.clone(),
    )
    .await?;
    status_updater
        .send(StateUpdateMessage::BrickletDisconnected {
            uid,
            endpoint: ip_addr,
        })
        .await?;
    Ok(())
}

async fn io_16_loop(
    termination_receiver: TestamentReceiver,
    channel_settings: [ChannelSetting; 16],
    button_event_stream: impl Stream<Item = IoMessage> + Sized + Unpin,
    status_updater: mpsc::Sender<StateUpdateMessage>,
) -> Result<(), IoHandlerError> {
    let (rx, tx) = mpsc::channel(2);
    let mut channel_timer: [Option<JoinHandle<()>>; 16] = <[Option<JoinHandle<()>>; 16]>::default();
    let mut receiver = button_event_stream
        .merge(termination_receiver.send_on_terminate(IoMessage::Close))
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
    #[error("Cannot parse Uid")]
    UidParse(#[from] Base58Error),
    #[error("Cannot update device status")]
    StateUpdate(#[from] mpsc::error::SendError<StateUpdateMessage>),
}

#[cfg(test)]
mod test {
    use crate::devices::io_handler::{ByteMaskIterator, IoMessage};

    #[test]
    fn test_byte_mask() {
        assert_eq!(
            vec![IoMessage::Press(0)],
            Iterator::collect::<Vec<_>>(ByteMaskIterator {
                value: 0b00000001,
                mask: 0b00000001,
                index: 0,
            })
        );
        assert_eq!(
            vec![
                IoMessage::Release(8),
                IoMessage::Press(9),
                IoMessage::Release(10),
                IoMessage::Press(11),
                IoMessage::Release(12),
                IoMessage::Press(13),
                IoMessage::Release(14),
                IoMessage::Press(15),
            ],
            Iterator::collect::<Vec<_>>(ByteMaskIterator {
                value: 0b10101010,
                mask: 0b11111111,
                index: 8,
            })
        );
    }
}
