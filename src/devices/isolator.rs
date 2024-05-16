use std::time::Duration;

use log::error;
use tinkerforge_async::{error::TinkerforgeError, isolator::IsolatorBricklet};
use tokio::{
    sync::mpsc::{self, Receiver},
    time::sleep,
};
use tokio::sync::mpsc::Sender;

use crate::{
    metrics::{report_device_temperature, report_spitf_error_counters},
    terminator::LifeLineEnd,
};
use crate::data::state::StateUpdateMessage;

pub fn handle_isolator(
    bricklet: IsolatorBricklet,
    status_updater: Sender<StateUpdateMessage>,
) -> LifeLineEnd {
    let (end1, end2) = LifeLineEnd::create();
    let poll_isolator = bricklet.clone();
    let (termination_tx, termination_rx) = mpsc::channel(1);
    end2.update_on_terminate((), termination_tx);
    tokio::spawn(async move {
        match isolator_poll(poll_isolator, termination_rx, status_updater).await {
            Ok(_) => {}
            Err(error) => {
                error!("Cannot poll Isolator Bricklet: {error}");
            }
        }
        drop(end2);
    });
    end1
}
/*
enum IsolatorEvent {
    Terminated,
    Statistics(StatisticsCallback),
}*/

async fn isolator_poll(
    mut isolator_bricklet: IsolatorBricklet,
    termination_rx: Receiver<()>,
    status_updater: Sender<StateUpdateMessage>,
) -> Result<(), TinkerforgeError> {
    let uid = isolator_bricklet.uid();
    while termination_rx.is_empty() && !termination_rx.is_closed() {
        let downstream_counter = isolator_bricklet.get_isolator_spitfp_error_count().await?;
        report_spitf_error_counters(
            &status_updater,
            uid,
            Some('z'),
            downstream_counter.error_count_ack_checksum,
            downstream_counter.error_count_message_checksum,
            downstream_counter.error_count_frame,
            downstream_counter.error_count_overflow,
        )
        .await;
        let upstream_counter = isolator_bricklet.get_spitfp_error_count().await?;
        report_spitf_error_counters(
            &status_updater,
            uid,
            None,
            upstream_counter.error_count_ack_checksum,
            upstream_counter.error_count_message_checksum,
            upstream_counter.error_count_frame,
            upstream_counter.error_count_overflow,
        )
        .await;
        let temperature = isolator_bricklet.get_chip_temperature().await?;
        report_device_temperature(uid, temperature as f64);
        sleep(Duration::from_secs(3600)).await;
    }
    Ok(())
}
