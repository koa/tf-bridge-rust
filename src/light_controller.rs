use std::hash::Hash;
use std::num::Saturating;

use log::error;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::SendError;
use tokio::task::{AbortHandle, JoinHandle};
use tokio::time::sleep;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use crate::registry::{BrightnessKey, ButtonState, DualButtonKey, DualButtonLayout, EventRegistry};

pub async fn dual_input_dimmer(
    event_registry: EventRegistry,
    input: DualButtonKey,
    output: BrightnessKey,
) -> AbortHandle {
    tokio::spawn(async move {
        if let Err(error) = dual_input_dimmer_task(event_registry, input, output).await {
            error!("Failed dual input dimmer: {error}")
        }
    })
    .abort_handle()
}

enum DimmerEvent<L: Copy + Eq + Hash> {
    ButtonState(ButtonState<L>),
    KeepPressing(L),
}

async fn dual_input_dimmer_task(
    event_registry: EventRegistry,
    input: DualButtonKey,
    output: BrightnessKey,
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

    let mut input_stream = event_registry
        .dual_button_stream(input)
        .await
        .map(DimmerEvent::ButtonState)
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
                if let Some(handle) = dimm_timer_handle.take() {
                    handle.abort();
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
                        sleep(core::time::Duration::from_millis(10)).await;
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
        }
    }
    Ok(())
}
