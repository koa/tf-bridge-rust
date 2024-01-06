use futures::Stream;
use log::error;
use tokio::{sync::mpsc, task::AbortHandle};
use tokio_stream::StreamExt;

use crate::data::registry::{
    ButtonState, EventRegistry, SingleButtonKey, SingleButtonLayout, SwitchOutputKey,
};

pub async fn ring_controller(
    event_registry: &EventRegistry,
    input: SingleButtonKey,
    output: SwitchOutputKey,
) -> AbortHandle {
    let input_stream = event_registry
        .single_button_stream(input)
        .await
        .map(ActionMessage::Button);
    let sender = event_registry.switch_sender(output).await;
    tokio::spawn(async move {
        if let Err(error) = ring_task(input_stream, sender).await {
            error!("Failed handle ring: {error}")
        }
    })
    .abort_handle()
}

async fn ring_task(
    mut input: impl Stream<Item = ActionMessage> + Unpin,
    output: mpsc::Sender<bool>,
) -> Result<(), mpsc::error::SendError<bool>> {
    let mut button_pressed = false;
    while let Some(event) = input.next().await {
        match event {
            ActionMessage::Button(ButtonState::ShortPressStart(_)) => {
                if !button_pressed {
                    output.send(true).await?;
                }
                button_pressed = true;
            }
            ActionMessage::Button(ButtonState::Released) => {
                if button_pressed {
                    output.send(false).await?;
                }
                button_pressed = false;
            }
            ActionMessage::Button(ButtonState::LongPressStart(_)) => {}
        }
    }
    Ok(())
}

enum ActionMessage {
    Button(ButtonState<SingleButtonLayout>),
}
