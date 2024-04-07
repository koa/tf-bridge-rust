use lazy_static::lazy_static;
use prometheus::{
    GaugeVec, IntGaugeVec, register_gauge_vec,
    register_int_gauge_vec,
};
use tinkerforge_async::base58::Uid;

lazy_static! {
    static ref ACK_CHECKSUM_ERROR_COUNTER: IntGaugeVec = register_int_gauge_vec!(
        "tinkerforge_spitf_error_count_ack_checksum",
        "ACK Checksum error counters of a thinkerforge device",
        &["uid", "port"]
    )
    .expect("Cannot initialize prometheus metric");
    static ref MESSAGE_CHECKSUM_ERROR_COUNTER: IntGaugeVec = register_int_gauge_vec!(
        "tinkerforge_spitf_error_count_message_checksum",
        "Message Checksum error counters of a thinkerforge device",
        &["uid", "port"]
    )
    .expect("Cannot initialize prometheus metric");
    static ref FRAME_ERROR_COUNTER: IntGaugeVec = register_int_gauge_vec!(
        "tinkerforge_spitf_error_count_frame",
        "Frame error counters of a thinkerforge device",
        &["uid", "port"]
    )
    .expect("Cannot initialize prometheus metric");
    static ref OVERFLOW_ERROR_COUNTER: IntGaugeVec = register_int_gauge_vec!(
        "tinkerforge_spitf_error_count_overflow",
        "Overflow error counters of a thinkerforge device",
        &["uid", "port"]
    )
    .expect("Cannot initialize prometheus metric");
    static ref DEVICE_CURRENT: GaugeVec = register_gauge_vec!(
        "tinkerforge_device_current",
        "Measured current of a tinkerforge device",
        &["uid"]
    )
    .expect("Cannot initialize prometheus metric");
    static ref DEVICE_VOLTAGE: GaugeVec = register_gauge_vec!(
        "tinkerforge_device_voltage",
        "Measured voltage of a tinkerforge device",
        &["uid"]
    )
    .expect("Cannot initialize prometheus metric");
    static ref DEVICE_ETHERNET_TX_COUNTER: IntGaugeVec = register_int_gauge_vec!(
        "tinkerforge_device_ethernet_tx",
        "Count of sent bytes",
        &["uid"]
    )
    .expect("Cannot initialize prometheus metric");
    static ref DEVICE_ETHERNET_RX_COUNTER: IntGaugeVec = register_int_gauge_vec!(
        "tinkerforge_device_ethernet_rx",
        "Count of received bytes",
        &["uid"]
    )
    .expect("Cannot initialize prometheus metric");
    static ref DEVICE_TEMPERATURE: GaugeVec = register_gauge_vec!(
        "tinkerforge_device_temperature",
        "Measured temperature of a tinkerforge device",
        &["uid"]
    )
    .expect("Cannot initialize prometheus metric");
}

pub fn report_spitf_error_counters(
    device: Uid,
    port: Option<char>,
    error_count_ack_checksum: u32,
    error_count_message_checksum: u32,
    error_count_frame: u32,
    error_count_overflow: u32,
) {
    let uid_string = device.to_string();
    let mut str_buffer = [0; 4];
    let option = port.map(|ch| ch.encode_utf8(&mut str_buffer));
    let port_string: &str = option.as_deref().unwrap_or("");
    let labels = &[uid_string.as_str(), port_string];
    ACK_CHECKSUM_ERROR_COUNTER
        .with_label_values(labels)
        .set(error_count_ack_checksum.into());
    MESSAGE_CHECKSUM_ERROR_COUNTER
        .with_label_values(labels)
        .set(error_count_message_checksum.into());
    FRAME_ERROR_COUNTER
        .with_label_values(labels)
        .set(error_count_frame.into());
    OVERFLOW_ERROR_COUNTER
        .with_label_values(labels)
        .set(error_count_overflow.into())
}

pub fn report_current(device: Uid, current: f64) {
    let uid_string = device.to_string();
    DEVICE_CURRENT
        .with_label_values(&[uid_string.as_str()])
        .set(current);
}

pub fn report_voltage(device: Uid, voltage: f64) {
    let uid_string = device.to_string();
    DEVICE_VOLTAGE
        .with_label_values(&[uid_string.as_str()])
        .set(voltage);
}

pub fn report_ethernet_traffic(device: Uid, tx_count: u32, rx_count: u32) {
    let uid_string = device.to_string();
    DEVICE_ETHERNET_RX_COUNTER
        .with_label_values(&[uid_string.as_str()])
        .set(rx_count as i64);
    DEVICE_ETHERNET_TX_COUNTER
        .with_label_values(&[uid_string.as_str()])
        .set(tx_count as i64);
}

pub fn report_device_temperature(device: Uid, temperature: f64) {
    let uid_string = device.to_string();
    DEVICE_TEMPERATURE
        .with_label_values(&[uid_string.as_str()])
        .set(temperature);
}
