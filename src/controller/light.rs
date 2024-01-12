use std::{hash::Hash, num::Saturating, time::Duration};

use futures::stream::SelectAll;
use log::error;
use tokio::{
    sync::mpsc::{self, error::SendError, Sender},
    task::AbortHandle,
    time::sleep,
};
use tokio_stream::{wrappers::ReceiverStream, Stream, StreamExt};

use crate::{
    data::registry::{
        BrightnessKey, ButtonState, DualButtonKey, DualButtonLayout, EventRegistry,
        SingleButtonKey, SingleButtonLayout, SwitchOutputKey,
    },
    terminator::JoinHandleTerminator,
    util::optional_stream,
};

pub async fn dual_input_dimmer(
    event_registry: &EventRegistry,
    inputs: &[DualButtonKey],
    output: BrightnessKey,
    auto_switch_off_time: Duration,
    presences: &[SingleButtonKey],
) -> AbortHandle {
    let current_brightness = event_registry
        .brightness_stream(output)
        .await
        .next()
        .await
        .unwrap_or_default();
    let sender = event_registry.brightness_sender(output).await;
    let input_stream = merge_dual_buttons_and_presences(event_registry, inputs, presences).await;
    tokio::spawn(async move {
        if let Err(error) = dual_input_dimmer_task(
            input_stream,
            auto_switch_off_time,
            current_brightness,
            sender,
        )
        .await
        {
            error!("Failed dual input dimmer: {error}")
        }
    })
    .abort_handle()
}

async fn merge_dual_buttons_and_presences(
    event_registry: &EventRegistry,
    inputs: &[DualButtonKey],
    presences: &[SingleButtonKey],
) -> impl Stream<Item = DimmerEvent<DualButtonLayout>> + Unpin {
    let mut button_streams = SelectAll::new();
    for input in inputs {
        button_streams.push(
            event_registry
                .dual_button_stream(*input)
                .await
                .map(DimmerEvent::ButtonState),
        );
    }
    button_streams.merge(create_presences_stream(event_registry, presences).await)
}

async fn create_presences_stream<L: Copy + Eq + Hash>(
    event_registry: &EventRegistry,
    presences: &[SingleButtonKey],
) -> impl Stream<Item = DimmerEvent<L>> + Unpin {
    let mut presence_streams = SelectAll::new();
    for presence_key in presences {
        presence_streams.push(
            event_registry
                .single_button_stream(*presence_key)
                .await
                .filter_map(|s| match s {
                    ButtonState::ShortPressStart(_) => Some(DimmerEvent::<L>::PresenceDetected),
                    _ => None,
                }),
        );
    }
    presence_streams
}
/*
async fn create_presence_stream<L: Copy + Eq + Hash>(
    event_registry: &EventRegistry,
    presence_key: Option<SingleButtonKey>,
) -> impl Stream<Item = DimmerEvent<L>> + Sized {
    optional_stream(presence_key.map(|k| event_registry.single_button_stream(k)))
        .await
        .filter_map(|s| match s {
            ButtonState::ShortPressStart(_) => Some(DimmerEvent::<L>::PresenceDetected),
            _ => None,
        })
}*/

pub async fn dual_input_switch(
    event_registry: &EventRegistry,
    inputs: &[DualButtonKey],
    output: SwitchOutputKey,
    auto_switch_off_time: Duration,
    presences: &[SingleButtonKey],
) -> AbortHandle {
    let current_state = event_registry
        .switch_stream(output)
        .await
        .next()
        .await
        .unwrap_or_default();
    let sender = event_registry.switch_sender(output).await;
    let input_stream = merge_dual_buttons_and_presences(event_registry, inputs, presences).await;
    tokio::spawn(async move {
        if let Err(error) =
            dual_input_switch_task(auto_switch_off_time, current_state, sender, input_stream).await
        {
            error!("Failed dual input switch: {error}")
        }
    })
    .abort_handle()
}

pub async fn motion_detector(
    event_registry: &EventRegistry,
    inputs: &[SingleButtonKey],
    output: SwitchOutputKey,
    switch_off_time: Duration,
) -> AbortHandle {
    let sender = event_registry.switch_sender(output).await;
    let current_value = event_registry.switch_stream(output).await.next().await;
    let input_stream = create_presences_stream(event_registry, inputs).await;
    tokio::spawn(async move {
        if let Err(error) =
            motion_detector_task(switch_off_time, sender, input_stream, current_value).await
        {
            error!("Failed motion detector: {error}")
        }
    })
    .abort_handle()
}
pub async fn motion_detector_dimmer(
    event_registry: &EventRegistry,
    inputs: &[SingleButtonKey],
    brightness: Option<BrightnessKey>,
    output: BrightnessKey,
    switch_off_time: Duration,
) -> AbortHandle {
    let current_brightness = event_registry.brightness_stream(output).await.next().await;
    let sender = event_registry.brightness_sender(output).await;
    let input_stream = create_presences_stream(event_registry, inputs).await.merge(
        optional_stream(brightness.map(|k| event_registry.brightness_stream(k)))
            .await
            .map(DimmerEvent::SetBrightness),
    );
    tokio::spawn(async move {
        if let Err(error) =
            motion_detector_dimmer_task(switch_off_time, sender, input_stream, current_brightness)
                .await
        {
            error!("Failed motion detector: {error}")
        }
    })
    .abort_handle()
}

async fn motion_detector_dimmer_task(
    auto_switch_off_time: Duration,
    output_sender: Sender<Saturating<u8>>,
    input_stream: impl Stream<Item = DimmerEvent<SingleButtonLayout>> + Sized + Unpin,
    current_brightness: Option<Saturating<u8>>,
) -> Result<(), SendError<Saturating<u8>>> {
    let (tx, rx) = mpsc::channel(2);

    let mut timer_handle = None::<JoinHandleTerminator<()>>;
    let mut on_brightness = Saturating(255);
    let mut light_enabled = false;
    if let Some(br) = current_brightness {
        if br.0 > 0 {
            light_enabled = true;
            on_brightness = br;
            start_switchoff_timer(auto_switch_off_time, &mut timer_handle, &tx);
        } else {
            light_enabled = false;
        }
    }
    let mut input_stream = input_stream.merge(ReceiverStream::new(rx));
    while let Some(event) = input_stream.next().await {
        match event {
            DimmerEvent::ButtonState(_) => {}
            DimmerEvent::KeepPressing(_) => {}
            DimmerEvent::AutoSwitchOff => {
                output_sender.send(Saturating(0)).await?;
                light_enabled = false;
            }
            DimmerEvent::PresenceDetected => {
                output_sender.send(on_brightness).await?;
                light_enabled = true;
                start_switchoff_timer(auto_switch_off_time, &mut timer_handle, &tx);
            }
            DimmerEvent::SetBrightness(br) => {
                on_brightness = br;
                if light_enabled {
                    output_sender.send(on_brightness).await?;
                }
            }
        }
    }
    Ok(())
}

fn start_switchoff_timer<T: Copy + Eq + Hash + Send + 'static>(
    auto_switch_off_time: Duration,
    timer_handle: &mut Option<JoinHandleTerminator<()>>,
    tx: &Sender<DimmerEvent<T>>,
) {
    let tx = tx.clone();
    timer_handle.replace(
        tokio::spawn(async move {
            loop {
                sleep(auto_switch_off_time).await;
                if let Err(error) = tx.send(DimmerEvent::<T>::AutoSwitchOff).await {
                    error!("Error sending switchoff: {error}");
                    break;
                }
            }
        })
        .into(),
    );
}

async fn motion_detector_task(
    auto_switch_off_time: Duration,
    output_sender: Sender<bool>,
    input_stream: impl Stream<Item = DimmerEvent<SingleButtonLayout>> + Sized + Unpin,
    current_value: Option<bool>,
) -> Result<(), SendError<bool>> {
    let (tx, rx) = mpsc::channel(2);

    let mut timer_handle = None::<JoinHandleTerminator<()>>;
    if let Some(current_state) = current_value {
        if current_state {
            start_switchoff_timer(auto_switch_off_time, &mut timer_handle, &tx);
        }
    }
    let mut input_stream = input_stream.merge(ReceiverStream::new(rx));
    while let Some(event) = input_stream.next().await {
        match event {
            DimmerEvent::ButtonState(_) => {}
            DimmerEvent::KeepPressing(_) => {}
            DimmerEvent::AutoSwitchOff => {
                output_sender.send(false).await?;
            }
            DimmerEvent::PresenceDetected => {
                output_sender.send(true).await?;
                start_switchoff_timer(auto_switch_off_time, &mut timer_handle, &tx);
            }
            DimmerEvent::SetBrightness(_) => {}
        }
    }
    Ok(())
}

enum DimmerEvent<L: Copy + Eq + Hash> {
    ButtonState(ButtonState<L>),
    KeepPressing(L),
    AutoSwitchOff,
    PresenceDetected,
    SetBrightness(Saturating<u8>),
}

async fn dual_input_dimmer_task(
    input_stream: impl Stream<Item = DimmerEvent<DualButtonLayout>> + Unpin,
    auto_switch_off_time: Duration,
    mut current_brightness: Saturating<u8>,
    sender: Sender<Saturating<u8>>,
) -> Result<(), SendError<Saturating<u8>>> {
    let (tx, rx) = mpsc::channel(2);

    let mut last_on_brightness = current_brightness;
    let mut is_long_press = false;
    let mut last_button = None;
    let mut dimm_timer_handle = None::<JoinHandleTerminator<()>>;
    if current_brightness.0 > 0 {
        start_switchoff_timer(auto_switch_off_time, &mut dimm_timer_handle, &tx);
    }

    let mut input_stream = input_stream.merge(ReceiverStream::new(rx));
    while let Some(event) = input_stream.next().await {
        match event {
            DimmerEvent::ButtonState(ButtonState::Released) => {
                if !is_long_press {
                    match last_button {
                        Some(DualButtonLayout::Up) => {
                            if last_on_brightness.0 > 0 {
                                current_brightness = last_on_brightness;
                            } else {
                                current_brightness = Saturating(255);
                                last_on_brightness = current_brightness;
                            }
                            sender.send(current_brightness).await?;
                        }
                        Some(DualButtonLayout::Down) => {
                            current_brightness = Saturating(0);
                            sender.send(current_brightness).await?;
                        }
                        None => {}
                    }
                } else {
                    last_on_brightness = current_brightness;
                }
                last_button = None;
                if current_brightness.0 > 0 {
                    start_switchoff_timer(auto_switch_off_time, &mut dimm_timer_handle, &tx);
                } else {
                    dimm_timer_handle.take();
                }
            }
            DimmerEvent::ButtonState(ButtonState::ShortPressStart(button)) => {
                is_long_press = false;
                last_button = Some(button);
            }
            DimmerEvent::ButtonState(ButtonState::LongPressStart(button)) => {
                is_long_press = true;
                last_button = Some(button);
                let tx = tx.clone();
                dimm_timer_handle.replace(
                    tokio::spawn(async move {
                        loop {
                            sleep(Duration::from_millis(10)).await;
                            if let Err(error) = tx.send(DimmerEvent::KeepPressing(button)).await {
                                error!("Error sending dim events: {error}");
                                break;
                            }
                        }
                    })
                    .into(),
                );
            }
            DimmerEvent::KeepPressing(button) => {
                match button {
                    DualButtonLayout::Up => {
                        current_brightness += 1;
                    }
                    DualButtonLayout::Down => {
                        current_brightness -= 1;
                    }
                }
                sender.send(current_brightness).await?
            }
            DimmerEvent::AutoSwitchOff => {
                current_brightness = Saturating(0);
                sender.send(current_brightness).await?;
            }
            DimmerEvent::PresenceDetected => {
                if current_brightness.0 > 0 {
                    start_switchoff_timer(auto_switch_off_time, &mut dimm_timer_handle, &tx);
                }
            }
            DimmerEvent::SetBrightness(_) => {}
        }
    }
    Ok(())
}

async fn dual_input_switch_task(
    auto_switch_off_time: Duration,
    mut current_state: bool,
    sender: Sender<bool>,
    input_stream: impl Stream<Item = DimmerEvent<DualButtonLayout>> + Sized + Unpin,
) -> Result<(), SendError<bool>> {
    let (tx, rx) = mpsc::channel(2);
    let mut input_stream = input_stream.merge(ReceiverStream::new(rx));

    let mut switch_timer_handle = None::<JoinHandleTerminator<()>>;
    if current_state {
        start_switchoff_timer(auto_switch_off_time, &mut switch_timer_handle, &tx);
    }

    while let Some(event) = input_stream.next().await {
        match event {
            DimmerEvent::ButtonState(ButtonState::ShortPressStart(button)) => {
                current_state = match button {
                    DualButtonLayout::Up => true,
                    DualButtonLayout::Down => false,
                };
                sender.send(current_state).await?;
                if current_state {
                    start_switchoff_timer(auto_switch_off_time, &mut switch_timer_handle, &tx);
                } else {
                    switch_timer_handle.take();
                }
            }
            DimmerEvent::ButtonState(_) => {}
            DimmerEvent::KeepPressing(_) => {}
            DimmerEvent::AutoSwitchOff => {
                current_state = false;
                sender.send(false).await?;
            }
            DimmerEvent::PresenceDetected => {
                if current_state {
                    start_switchoff_timer(auto_switch_off_time, &mut switch_timer_handle, &tx);
                } else {
                    switch_timer_handle.take();
                }
            }
            DimmerEvent::SetBrightness(_) => {}
        }
    }
    Ok(())
}
