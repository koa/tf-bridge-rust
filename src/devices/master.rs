use std::time::Duration;

use log::{error, info};
use tinkerforge_async::{error::TinkerforgeError, master::MasterBrick};
use tokio::{
    sync::mpsc::{
        self,
        Receiver,
    },
    time::sleep,
};
use tokio_stream::StreamExt;

use crate::{
    metrics::{
        report_current,
        report_device_temperature,
        report_ethernet_traffic,
        report_spitf_error_counters,
        report_voltage,
    },
    terminator::LifeLineEnd,
};

pub fn handle_master(
    bricklet: MasterBrick,
) -> LifeLineEnd {
    let (end1, end2) = LifeLineEnd::create();
    let poll_master = bricklet.clone();
    let (termination_tx, termination_rx) = mpsc::channel(1);
    end2.update_on_terminate((), termination_tx);
    tokio::spawn(async move {
        match master_poll(poll_master, termination_rx).await {
            Ok(_) => {}
            Err(error) => {
                error!("Cannot poll MasterBrick: {error}");
            }
        }
    });
    tokio::spawn(async move {
        match master_loop(bricklet, &end2).await {
            Err(error) => {
                error!("Cannot communicate with MasterBrick: {error}");
            }
            Ok(_) => {
                info!("MasterBrick done");
            }
        }
        drop(end2);
    });

    end1
}

enum MasterChangeEvent {
    Terminated,
    CurrentUpdated(f64),
    VoltageUpdated(f64),
}

async fn master_loop(
    mut master: MasterBrick,
    life_line: &LifeLineEnd,
) -> Result<(), TinkerforgeError> {
    master.set_stack_current_callback_period(10000).await?;
    master.set_stack_voltage_callback_period(10000).await?;
    let uid = master.uid();

    let mut event_stream = life_line
        .send_on_terminate(MasterChangeEvent::Terminated)
        .merge(
            master
                .stack_current_stream()
                .await
                .map(|v| (v as f64) / 1000.0)
                .map(MasterChangeEvent::CurrentUpdated),
        )
        .merge(
            master
                .stack_voltage_stream()
                .await
                .map(|v| (v as f64) / 1000.0)
                .map(MasterChangeEvent::VoltageUpdated),
        );
    while let Some(event) = event_stream.next().await {
        match event {
            MasterChangeEvent::CurrentUpdated(current) => {
                report_current(uid, current);
            }
            MasterChangeEvent::VoltageUpdated(voltage) => {
                report_voltage(uid, voltage);
            }
            MasterChangeEvent::Terminated => {
                break;
            }
        }
    }
    Ok(())
}

async fn master_poll(
    mut poll_master: MasterBrick,
    termination_rx: Receiver<()>,
) -> Result<(), TinkerforgeError> {
    let uid = poll_master.uid();
    while termination_rx.is_empty() && !termination_rx.is_closed() {
        let ethernet_status = poll_master.get_ethernet_status().await?;
        report_ethernet_traffic(uid, ethernet_status.tx_count, ethernet_status.rx_count);
        for port in ['a', 'b', 'c', 'd'] {
            let counters = poll_master.get_spitfp_error_count(port).await?;
            report_spitf_error_counters(
                uid,
                Some(port),
                counters.error_count_ack_checksum,
                counters.error_count_message_checksum,
                counters.error_count_frame,
                counters.error_count_overflow,
            );
        }
        let temperature = poll_master.get_chip_temperature().await?;
        report_device_temperature(uid, temperature as f64 / 10.0);
        sleep(Duration::from_secs(10)).await;
    }
    Ok(())
}
