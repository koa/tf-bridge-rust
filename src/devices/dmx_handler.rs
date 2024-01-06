use std::num::Saturating;

use futures::stream::SelectAll;
use log::{error, info};
use sub_array::SubArray;
use tinkerforge_async::{
    dmx_bricklet::{DmxBricklet, DMX_BRICKLET_DMX_MODE_MASTER},
    error::TinkerforgeError,
};
use tokio::sync::{mpsc, mpsc::Sender};
use tokio_stream::{wrappers::ReceiverStream, Stream, StreamExt};
use tokio_util::either::Either;

use crate::data::{registry::EventRegistry, wiring::DmxConfigEntry};

pub async fn handle_dmx(
    bricklet: DmxBricklet,
    event_registry: EventRegistry,
    config: &[DmxConfigEntry],
) -> Sender<()> {
    let (tx, rx) = mpsc::channel(1);
    let mut streams = SelectAll::new();
    for config_entry in config.iter().cloned() {
        streams.push(match config_entry {
            DmxConfigEntry::Dimm { register, channel } => Either::Left(Either::Left(
                event_registry
                    .brightness_stream(register)
                    .await
                    .map(move |v| DmxCommand::single(channel, v.0)),
            )),
            DmxConfigEntry::Switch { register, channel } => Either::Left(Either::Right(
                event_registry
                    .switch_stream(register)
                    .await
                    .map(move |v| DmxCommand::single(channel, if v { 255 } else { 0 })),
            )),
            DmxConfigEntry::DimmWhitebalance {
                brightness_register,
                whitebalance_register,
                warm_channel,
                cold_channel,
                warm_mireds,
                cold_mireds,
            } => {
                let mut current_brightness = Saturating(0);
                let mut current_whitebalance = Saturating((cold_mireds + warm_mireds) / 2);
                let min_mireds = Saturating(cold_mireds);
                let max_mireds = Saturating(warm_mireds);
                Either::Right(
                    event_registry
                        .brightness_stream(brightness_register)
                        .await
                        .map(DimmColorUpdate::Brightness)
                        .merge(
                            event_registry
                                .light_color_stream(whitebalance_register)
                                .await
                                .map(move |v| v.clamp(min_mireds, max_mireds))
                                .map(DimmColorUpdate::Color),
                        )
                        .map(move |event| {
                            match event {
                                DimmColorUpdate::Brightness(br) => {
                                    current_brightness = br;
                                }
                                DimmColorUpdate::Color(c) => current_whitebalance = c,
                            };
                            (current_brightness, current_whitebalance)
                        })
                        .map(move |(brightness, wb)| {
                            let warm_part = wb - Saturating(cold_mireds);
                            let cold_part = Saturating(warm_mireds) - wb;
                            let stretch =
                                1.0 / warm_part.max(cold_part).0 as f32 * brightness.0 as f32;
                            DmxCommand::dual(
                                warm_channel,
                                (warm_part.0 as f32 * stretch) as u8,
                                cold_channel,
                                (cold_part.0 as f32 * stretch) as u8,
                            )
                        }),
                )
            }
        });
    }
    let events = streams.merge(ReceiverStream::new(rx).map(|_| DmxCommand::Exit));

    tokio::spawn(async move {
        match dmx_loop(bricklet, events).await {
            Err(error) => {
                error!("Cannot communicate with DmxBricklet: {error}");
            }
            Ok(_) => {
                info!("DmxBricklet done");
            }
        }
    });
    tx
}

enum DmxCommand {
    SetSingleChannel {
        channel: u16,
        value: u8,
    },
    SetDualChannel {
        lower_channel: u16,
        lower_value: u8,
        higher_channel: u16,
        higher_value: u8,
    },
    Exit,
}

impl DmxCommand {
    fn single(channel: u16, value: u8) -> Self {
        Self::SetSingleChannel { channel, value }
    }
    fn dual(channel1: u16, value1: u8, channel2: u16, value2: u8) -> Self {
        let (lower_channel, lower_value, higher_channel, higher_value) = if channel1 < channel2 {
            (channel1, value1, channel2, value2)
        } else {
            (channel2, value2, channel1, value1)
        };
        Self::SetDualChannel {
            lower_channel,
            lower_value,
            higher_channel,
            higher_value,
        }
    }
}

enum DimmColorUpdate {
    Brightness(Saturating<u8>),
    Color(Saturating<u16>),
}

const DMX_PAKET_SIZE: u16 = 60;

async fn dmx_loop<St: Stream<Item = DmxCommand> + Unpin>(
    mut bricklet: DmxBricklet,
    mut stream: St,
) -> Result<(), TinkerforgeError> {
    bricklet.set_dmx_mode(DMX_BRICKLET_DMX_MODE_MASTER).await?;
    let mut channel_values = [0u8; 480];
    while let Some(event) = stream.next().await {
        //let start_time = SystemTime::now();
        match event {
            DmxCommand::SetSingleChannel { channel, value } => {
                if let Some(entry) = channel_values.get_mut(channel as usize) {
                    if *entry == value {
                        continue;
                    }
                    *entry = value;
                    let offset = channel - (channel % DMX_PAKET_SIZE);
                    bricklet
                        .write_frame_low_level(
                            DMX_PAKET_SIZE,
                            offset,
                            channel_values.sub_array_ref(offset as usize),
                        )
                        .await?;
                }
            }
            DmxCommand::SetDualChannel {
                lower_channel,
                lower_value,
                higher_channel,
                higher_value,
            } => {
                let lower_changed =
                    if let Some(entry) = channel_values.get_mut(lower_channel as usize) {
                        if *entry != lower_value {
                            *entry = lower_value;
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                let higher_changed =
                    if let Some(entry) = channel_values.get_mut(higher_channel as usize) {
                        if *entry != higher_value {
                            *entry = higher_value;
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                match match (lower_changed, higher_changed) {
                    (false, false) => None,
                    (true, false) => Some((lower_channel, None)),
                    (false, true) => Some((higher_channel, None)),
                    (true, true) => Some((lower_channel, Some(higher_channel))),
                } {
                    None => {}
                    Some((channel, None)) => {
                        let offset = channel - (channel % DMX_PAKET_SIZE);
                        bricklet
                            .write_frame_low_level(
                                DMX_PAKET_SIZE,
                                offset,
                                channel_values.sub_array_ref(offset as usize),
                            )
                            .await?;
                    }
                    Some((lower_channel, Some(upper_channel))) => {
                        let span = upper_channel - lower_channel;
                        if span < DMX_PAKET_SIZE {
                            let offset = (Saturating(upper_channel) - Saturating(DMX_PAKET_SIZE)).0;
                            bricklet
                                .write_frame_low_level(
                                    DMX_PAKET_SIZE,
                                    offset,
                                    channel_values.sub_array_ref(offset as usize),
                                )
                                .await?;
                        } else {
                            let offset = lower_channel - (lower_channel % DMX_PAKET_SIZE);
                            bricklet
                                .write_frame_low_level(
                                    DMX_PAKET_SIZE,
                                    offset,
                                    channel_values.sub_array_ref(offset as usize),
                                )
                                .await?;
                            let offset = upper_channel - (upper_channel % DMX_PAKET_SIZE);
                            bricklet
                                .write_frame_low_level(
                                    DMX_PAKET_SIZE,
                                    offset,
                                    channel_values.sub_array_ref(offset as usize),
                                )
                                .await?;
                        }
                    }
                };
            }
            DmxCommand::Exit => break,
        }
        /*if let Ok(elapsed) = start_time.elapsed() {
            info!("DMX cycle: {elapsed:?}");
        }*/
    }
    Ok(())
}