use futures::{SinkExt, Stream};
use log::error;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::SendError;
use tokio::task::AbortHandle;
use tokio_stream::StreamExt;

use crate::data::registry::{EventRegistry, SwitchOutputKey, TemperatureKey};

pub async fn heat_controller(
    event_registry: &EventRegistry,
    current_value: TemperatureKey,
    target_value: TemperatureKey,
    output: SwitchOutputKey,
) -> AbortHandle {
    let input_stream = event_registry
        .temperature_stream(current_value)
        .await
        .map(HeatContollerMessage::UpdateCurrentTemperature)
        .merge(
            event_registry
                .temperature_stream(target_value)
                .await
                .map(HeatContollerMessage::UpdateTargetTemperature),
        );
    let sender = event_registry.switch_sender(output).await;
    tokio::spawn(async move {
        if let Err(error) = heat_task(input_stream, sender).await {
            error!("Failed dual input dimmer: {error}")
        }
    })
    .abort_handle()
}

async fn heat_task(
    mut input: impl Stream<Item = HeatContollerMessage> + Unpin,
    output: mpsc::Sender<bool>,
) -> Result<(), SendError<bool>> {
    let mut current_temperature = 21.0;
    let mut target_temperature = 21.0;
    while let Some(event) = input.next().await {
        match event {
            HeatContollerMessage::UpdateTargetTemperature(target) => {
                target_temperature = target;
            }
            HeatContollerMessage::UpdateCurrentTemperature(current) => {
                current_temperature = current;
            }
        };
        output
            .send(current_temperature < target_temperature)
            .await?;
    }
    Ok(())
}

enum HeatContollerMessage {
    UpdateTargetTemperature(f32),
    UpdateCurrentTemperature(f32),
}
