use std::time::Duration;
use std::{hash::Hash, num::Saturating};

use log::error;
use tokio::sync::mpsc::Sender;
use tokio::{
    sync::mpsc::{self, error::SendError},
    task::{AbortHandle, JoinHandle},
    time::sleep,
};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};

use crate::data::registry::{
    BrightnessKey, ButtonState, DualButtonKey, DualButtonLayout, EventRegistry, SingleButtonKey,
    SwitchOutputKey,
};
use crate::util::optional_stream;

pub async fn dual_input_dimmer(
    event_registry: EventRegistry,
    input: DualButtonKey,
    output: BrightnessKey,
    auto_switch_off_time: Duration,
    presence: Option<SingleButtonKey>,
) -> AbortHandle {
    tokio::spawn(async move {
        if let Err(error) = dual_input_dimmer_task(
            event_registry,
            input,
            output,
            auto_switch_off_time,
            presence,
        )
        .await
        {
            error!("Failed dual input dimmer: {error}")
        }
    })
    .abort_handle()
}

pub async fn dual_input_switch(
    event_registry: EventRegistry,
    input: DualButtonKey,
    output: SwitchOutputKey,
    auto_switch_off_time: Duration,
    presence: Option<SingleButtonKey>,
) -> AbortHandle {
    tokio::spawn(async move {
        if let Err(error) = dual_input_switch_task(
            event_registry,
            input,
            output,
            auto_switch_off_time,
            presence,
        )
        .await
        {
            error!("Failed dual input dimmer: {error}")
        }
    })
    .abort_handle()
}

enum DimmerEvent<L: Copy + Eq + Hash> {
    ButtonState(ButtonState<L>),
    KeepPressing(L),
    AutoSwitchOff,
    PresenceDetected,
}

async fn dual_input_dimmer_task(
    event_registry: EventRegistry,
    input: DualButtonKey,
    output: BrightnessKey,
    auto_switch_off_time: Duration,
    presence: Option<SingleButtonKey>,
) -> Result<(), SendError<Saturating<u8>>> {
    let mut current_brightness = event_registry
        .brightness_stream(output)
        .await
        .next()
        .await
        .unwrap_or_default();
    let mut last_on_brightness = current_brightness;
    let sender = event_registry.brightness_sender(output).await;
    let (tx, rx) = mpsc::channel(2);

    let presence_stream = optional_stream(presence.map(|k| event_registry.single_button_stream(k)))
        .await
        .filter_map(|s| match s {
            ButtonState::ShortPressStart(_) => {
                Some(DimmerEvent::<DualButtonLayout>::PresenceDetected)
            }
            _ => None,
        });
    let mut input_stream = event_registry
        .dual_button_stream(input)
        .await
        .map(DimmerEvent::ButtonState)
        .merge(presence_stream)
        .merge(ReceiverStream::new(rx));

    let mut is_long_press = false;
    let mut last_button = DualButtonLayout::UP;
    let mut dimm_timer_handle = None::<JoinHandle<()>>;

    while let Some(event) = input_stream.next().await {
        match event {
            DimmerEvent::ButtonState(ButtonState::Released) => {
                if !is_long_press {
                    match last_button {
                        DualButtonLayout::UP => {
                            if last_on_brightness.0 > 0 {
                                current_brightness = last_on_brightness;
                            } else {
                                current_brightness = Saturating(255);
                                last_on_brightness = current_brightness;
                            }
                        }
                        DualButtonLayout::DOWN => {
                            current_brightness = Saturating(0);
                        }
                    }
                    sender.send(current_brightness).await?;
                } else {
                    last_on_brightness = current_brightness;
                }
                if current_brightness.0 > 0 {
                    reset_auto_switchoff_timer(auto_switch_off_time, &tx, &mut dimm_timer_handle);
                } else {
                    clear_timer(&mut dimm_timer_handle)
                }
            }
            DimmerEvent::ButtonState(ButtonState::ShortPressStart(button)) => {
                is_long_press = false;
                last_button = button;
            }
            DimmerEvent::ButtonState(ButtonState::LongPressStart(button)) => {
                is_long_press = true;
                last_button = button;
                let tx = tx.clone();
                if let Some(handle) = dimm_timer_handle.replace(tokio::spawn(async move {
                    loop {
                        sleep(Duration::from_millis(10)).await;
                        if let Err(error) = tx.send(DimmerEvent::KeepPressing(button)).await {
                            error!("Error sending dim events: {error}");
                            break;
                        }
                    }
                })) {
                    handle.abort();
                }
            }
            DimmerEvent::KeepPressing(button) => {
                match button {
                    DualButtonLayout::UP => {
                        current_brightness += 1;
                    }
                    DualButtonLayout::DOWN => {
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
                    reset_auto_switchoff_timer(auto_switch_off_time, &tx, &mut dimm_timer_handle);
                }
            }
        }
    }
    Ok(())
}
async fn dual_input_switch_task(
    event_registry: EventRegistry,
    input: DualButtonKey,
    output: SwitchOutputKey,
    auto_switch_off_time: Duration,
    presence: Option<SingleButtonKey>,
) -> Result<(), SendError<bool>> {
    let mut current_state = event_registry
        .switch_stream(output)
        .await
        .next()
        .await
        .unwrap_or_default();
    let sender = event_registry.switch_sender(output).await;
    let (tx, rx) = mpsc::channel(2);

    let presence_stream = optional_stream(presence.map(|k| event_registry.single_button_stream(k)))
        .await
        .filter_map(|s| match s {
            ButtonState::ShortPressStart(_) => {
                Some(DimmerEvent::<DualButtonLayout>::PresenceDetected)
            }
            _ => None,
        });
    let mut input_stream = event_registry
        .dual_button_stream(input)
        .await
        .map(DimmerEvent::ButtonState)
        .merge(presence_stream)
        .merge(ReceiverStream::new(rx));

    let mut switch_timer_handle = None::<JoinHandle<()>>;

    while let Some(event) = input_stream.next().await {
        match event {
            DimmerEvent::ButtonState(ButtonState::ShortPressStart(button)) => {
                current_state = match button {
                    DualButtonLayout::UP => true,
                    DualButtonLayout::DOWN => false,
                };
                sender.send(current_state).await?;
                if current_state {
                    reset_auto_switchoff_timer(auto_switch_off_time, &tx, &mut switch_timer_handle);
                } else {
                    clear_timer(&mut switch_timer_handle);
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
                    reset_auto_switchoff_timer(auto_switch_off_time, &tx, &mut switch_timer_handle);
                } else {
                    clear_timer(&mut switch_timer_handle);
                }
            }
        }
    }
    Ok(())
}

fn reset_auto_switchoff_timer<T: Copy + Eq + Hash + Send + 'static>(
    auto_switch_off_time: Duration,
    tx: &Sender<DimmerEvent<T>>,
    timer_handle: &mut Option<JoinHandle<()>>,
) {
    let tx = tx.clone();
    if let Some(handle) = timer_handle.replace(tokio::spawn(async move {
        loop {
            sleep(auto_switch_off_time).await;
            if let Err(error) = tx.send(DimmerEvent::AutoSwitchOff).await {
                error!("Error sending dim events: {error}");
                break;
            }
        }
    })) {
        handle.abort();
    }
}
fn clear_timer(timer_handle: &mut Option<JoinHandle<()>>) {
    if let Some(handle) = timer_handle.take() {
        handle.abort();
    }
}
