use std::time::Duration;

use log::{error, info};
use tinkerforge_async::{
    error::TinkerforgeError,
    isolator::{
        IsolatorBricklet, SetStatisticsCallbackConfigurationRequest, StatisticsCallback,
    },
    master::MasterBrick,
};
use tokio::{
    sync::mpsc::{
        self,
        Receiver,
    },
    time::sleep,
};

use crate::{
    metrics::{report_device_temperature, report_spitf_error_counters},
    terminator::LifeLineEnd,
};

pub fn handle_isolator(bricklet: IsolatorBricklet) -> LifeLineEnd {
    let (end1, end2) = LifeLineEnd::create();
    let poll_isolator = bricklet.clone();
    let (termination_tx, termination_rx) = mpsc::channel(1);
    end2.update_on_terminate((), termination_tx);
    tokio::spawn(async move {
        match isolator_poll(poll_isolator, termination_rx).await {
            Ok(_) => {}
            Err(error) => {
                error!("Cannot poll Isolator Bricklet: {error}");
            }
        }
        drop(end2);
    });
    end1
}

enum IsolatorEvent {
    Terminated,
    Statistics(StatisticsCallback),
}

async fn isolator_poll(
    mut isolator_bricklet: IsolatorBricklet,
    termination_rx: Receiver<()>,
) -> Result<(), TinkerforgeError> {
    let uid = isolator_bricklet.uid();
    while termination_rx.is_empty() && !termination_rx.is_closed() {
        let downstream_counter = isolator_bricklet.get_isolator_spitfp_error_count().await?;
        report_spitf_error_counters(
            uid,
            Some('z'),
            downstream_counter.error_count_ack_checksum,
            downstream_counter.error_count_message_checksum,
            downstream_counter.error_count_frame,
            downstream_counter.error_count_overflow,
        );
        let upstream_counter = isolator_bricklet.get_spitfp_error_count().await?;
        report_spitf_error_counters(
            uid,
            None,
            upstream_counter.error_count_ack_checksum,
            upstream_counter.error_count_message_checksum,
            upstream_counter.error_count_frame,
            upstream_counter.error_count_overflow,
        );
        let temperature = isolator_bricklet.get_chip_temperature().await?;
        report_device_temperature(uid, temperature as f64);
        sleep(Duration::from_secs(10)).await;
    }
    Ok(())
}
