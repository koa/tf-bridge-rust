use chrono::{DateTime, Timelike, Utc};
use chrono_tz::Tz;
use log::error;
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering, collections::HashMap, future::Future, hash::Hash, num::Saturating,
    ops::DerefMut, sync::Arc, time::Duration,
};
use tokio::{sync::mpsc::Sender, sync::Mutex, time::sleep};
use tokio_stream::Stream;

use crate::data::{register::Register, DeviceInRoom, SubDeviceInRoom};

pub trait TypedKey {
    type Value;
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct ClockKey {
    pub resolution: ClockKeyResolution,
    pub tz: Tz,
}
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Ord, PartialOrd)]
pub enum ClockKeyResolution {
    Seconds,
    Minutes,
}
impl TypedKey for ClockKey {
    type Value = DateTime<Tz>;
}
impl PartialOrd for ClockKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for ClockKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.resolution
            .cmp(&other.resolution)
            .then(self.tz.name().cmp(other.tz.name()))
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Ord, PartialOrd)]
pub enum TemperatureKey {
    CurrentTemperature(DeviceInRoom),
    TargetTemperature(DeviceInRoom),
}
impl TypedKey for TemperatureKey {
    type Value = f32;
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Ord, PartialOrd)]
pub enum LightColorKey {
    Light(DeviceInRoom),
    TouchscreenController(DeviceInRoom),
}
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Ord, PartialOrd)]
pub enum BrightnessKey {
    Light(DeviceInRoom),
    TouchscreenController(DeviceInRoom),
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Ord, PartialOrd)]
pub enum SwitchOutputKey {
    Light(DeviceInRoom),
    Heat(DeviceInRoom),
    Bell(DeviceInRoom),
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Ord, PartialOrd)]
pub struct DualButtonKey(pub SubDeviceInRoom);
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Ord, PartialOrd)]
pub enum SingleButtonKey {
    Button(SubDeviceInRoom),
    MotionDetector(DeviceInRoom),
}
#[derive(Copy, Clone, Eq, PartialEq, Hash, Default, Debug, Serialize, Deserialize)]
pub enum ButtonState<B: Copy + Clone + Eq + Hash> {
    #[default]
    Released,
    ShortPressStart(B),
    LongPressStart(B),
}
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Ord, PartialOrd)]
pub enum DualButtonLayout {
    Up,
    Down,
}
#[derive(
    Copy, Clone, Eq, PartialEq, Hash, Debug, Default, Serialize, Deserialize, Ord, PartialOrd,
)]
pub struct SingleButtonLayout;
pub trait KeyAccess<K: TypedKey<Value = V>, V: Clone + Sync + Send + 'static> {
    async fn register_access<
        'a,
        'b,
        F: Fn(&'a Register<DateTime<Tz>>) -> FR + Send + 'a,
        R: 'b + Send,
        FR: Future<Output = R> + Send,
    >(
        &'a self,
        key: K,
        function: F,
    ) -> FR;
}

#[derive(Clone)]
pub struct EventRegistry {
    inner: Arc<Mutex<InnerEventRegistry>>,
}

/*
#[async_trait]
impl KeyAccess<ClockKey, DateTime<Local>> for EventRegistry {
    async fn register_access<
        'a,
        'b,
        F: Fn(&'a Register<DateTime<Local>>) -> FR + Send + 'a,
        R: 'b + Send,
        FR: Future<Output = R>,
    >(
        &'a self,
        key: ClockKey,
        function: F,
    ) -> FR {
        let guard = self.inner.lock().await;
        let result = function(&guard.clock_receiver);
        drop(guard);
        result
    }
}

 */
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug, Default)]
pub struct ValueSnapshots {
    temperatures: HashMap<TemperatureKey, f32>,
    light_colors: HashMap<LightColorKey, u16>,
    brightness: HashMap<BrightnessKey, u8>,
    output_switch: HashMap<SwitchOutputKey, bool>,
}

struct InnerEventRegistry {
    default_values: ValueSnapshots,
    clock_registers: HashMap<ClockKey, Register<DateTime<Tz>>>,
    temperature_registers: HashMap<TemperatureKey, Register<f32>>,
    light_color_registers: HashMap<LightColorKey, Register<Saturating<u16>>>,
    brightness_color: HashMap<BrightnessKey, Register<Saturating<u8>>>,
    dual_buttons: HashMap<DualButtonKey, Register<ButtonState<DualButtonLayout>>>,
    buttons: HashMap<SingleButtonKey, Register<ButtonState<SingleButtonLayout>>>,
    output_switch: HashMap<SwitchOutputKey, Register<bool>>,
}

impl InnerEventRegistry {
    fn take_snapshot(&self) -> ValueSnapshots {
        ValueSnapshots {
            temperatures: self
                .temperature_registers
                .iter()
                .map(|(key, register)| (*key, register.current_value()))
                .collect(),
            light_colors: self
                .light_color_registers
                .iter()
                .map(|(key, register)| (*key, register.current_value().0))
                .collect(),
            brightness: self
                .brightness_color
                .iter()
                .map(|(key, register)| (*key, register.current_value().0))
                .collect(),
            output_switch: self
                .output_switch
                .iter()
                .map(|(key, register)| (*key, register.current_value()))
                .collect(),
        }
    }
    fn temperature_register(&mut self, key: TemperatureKey) -> &mut Register<f32> {
        self.temperature_registers.entry(key).or_insert_with(|| {
            Register::new(
                self.default_values
                    .temperatures
                    .get(&key)
                    .copied()
                    .unwrap_or(21.0),
            )
        })
    }
    fn light_color_register(&mut self, key: LightColorKey) -> &mut Register<Saturating<u16>> {
        self.light_color_registers.entry(key).or_insert_with(|| {
            Register::new(Saturating(
                self.default_values
                    .light_colors
                    .get(&key)
                    .copied()
                    .unwrap_or(200),
            ))
        })
    }
    fn brightness_register(&mut self, key: BrightnessKey) -> &mut Register<Saturating<u8>> {
        self.brightness_color.entry(key).or_insert_with(|| {
            let option = self.default_values.brightness.get(&key).copied();
            Register::new(Saturating(option.unwrap_or(match key {
                BrightnessKey::Light(_) => 0,
                BrightnessKey::TouchscreenController(_) => 255,
            })))
        })
    }
    fn dual_button_register(
        &mut self,
        key: DualButtonKey,
    ) -> &mut Register<ButtonState<DualButtonLayout>> {
        self.dual_buttons.entry(key).or_default()
    }
    fn button_register(
        &mut self,
        key: SingleButtonKey,
    ) -> &mut Register<ButtonState<SingleButtonLayout>> {
        self.buttons.entry(key).or_default()
    }

    fn switch_register(&mut self, key: SwitchOutputKey) -> &mut Register<bool> {
        self.output_switch.entry(key).or_insert_with(|| {
            Register::new(
                self.default_values
                    .output_switch
                    .get(&key)
                    .copied()
                    .unwrap_or_default(),
            )
        })
    }
    fn clock(&mut self, clock_key: ClockKey) -> &mut Register<DateTime<Tz>> {
        self.clock_registers
            .entry(clock_key)
            .or_insert_with_key(|clock_key| {
                let step_size = match clock_key.resolution {
                    ClockKeyResolution::Minutes => Duration::from_secs(60).as_millis() as u32,
                    ClockKeyResolution::Seconds => Duration::from_secs(1).as_millis() as u32,
                };

                let clock_receiver = Register::new(Utc::now().with_timezone(&clock_key.tz));
                let sender = clock_receiver.sender();
                let tz = clock_key.tz;
                tokio::spawn(async move {
                    loop {
                        let time = Utc::now().with_timezone(&tz);
                        match sender.send(time).await {
                            Ok(()) => {}
                            Err(error) => {
                                error!("Cannot send wall clock: {error}")
                            }
                        }
                        let wait_time = step_size
                            - (time.timestamp_subsec_millis() + time.second() * 1000) % step_size;
                        sleep(Duration::from_millis(wait_time as u64)).await;
                    }
                });
                clock_receiver
            })
    }
}

impl EventRegistry {
    //pub async fn clock(&self) -> impl Stream<Item = DateTime<Local>> {
    // BroadcastStream::new(self.inner.lock().await.clock_receiver.resubscribe())
    //     .filter_map(|e| e.ok())
    //}
    pub fn new(default_values: Option<ValueSnapshots>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(InnerEventRegistry::new(default_values))),
        }
    }
    pub async fn take_snapshot(&self) -> ValueSnapshots {
        self.inner.lock().await.take_snapshot()
    }
    pub async fn clock(&self, key: ClockKey) -> impl Stream<Item = DateTime<Tz>> {
        self.inner
            .lock()
            .await
            .deref_mut()
            .clock(key)
            .stream()
            .await
    }
    pub async fn temperature_stream(&self, temperature: TemperatureKey) -> impl Stream<Item = f32> {
        self.inner
            .lock()
            .await
            .deref_mut()
            .temperature_register(temperature)
            .stream()
            .await
    }
    pub async fn temperature_sender(&self, temperature: TemperatureKey) -> Sender<f32> {
        self.inner
            .lock()
            .await
            .deref_mut()
            .temperature_register(temperature)
            .sender()
    }
    pub async fn light_color_stream(
        &self,
        light_color: LightColorKey,
    ) -> impl Stream<Item = Saturating<u16>> {
        self.inner
            .lock()
            .await
            .deref_mut()
            .light_color_register(light_color)
            .stream()
            .await
    }
    pub async fn light_color_sender(&self, light_color: LightColorKey) -> Sender<Saturating<u16>> {
        self.inner
            .lock()
            .await
            .deref_mut()
            .light_color_register(light_color)
            .sender()
    }
    pub async fn brightness_stream(
        &self,
        brightness_key: BrightnessKey,
    ) -> impl Stream<Item = Saturating<u8>> {
        self.inner
            .lock()
            .await
            .deref_mut()
            .brightness_register(brightness_key)
            .stream()
            .await
    }
    pub async fn brightness_sender(&self, brightness_key: BrightnessKey) -> Sender<Saturating<u8>> {
        self.inner
            .lock()
            .await
            .deref_mut()
            .brightness_register(brightness_key)
            .sender()
    }
    pub async fn dual_button_stream(
        &self,
        dual_button_key: DualButtonKey,
    ) -> impl Stream<Item = ButtonState<DualButtonLayout>> {
        self.inner
            .lock()
            .await
            .deref_mut()
            .dual_button_register(dual_button_key)
            .stream()
            .await
    }
    pub async fn dual_button_sender(
        &self,
        dual_button_key: DualButtonKey,
    ) -> Sender<ButtonState<DualButtonLayout>> {
        self.inner
            .lock()
            .await
            .dual_button_register(dual_button_key)
            .sender()
    }
    pub async fn single_button_stream(
        &self,
        single_button_key: SingleButtonKey,
    ) -> impl Stream<Item = ButtonState<SingleButtonLayout>> {
        self.inner
            .lock()
            .await
            .deref_mut()
            .button_register(single_button_key)
            .stream()
            .await
    }
    pub async fn single_button_sender(
        &self,
        single_button_key: SingleButtonKey,
    ) -> Sender<ButtonState<SingleButtonLayout>> {
        self.inner
            .lock()
            .await
            .button_register(single_button_key)
            .sender()
    }
    pub async fn switch_stream(
        &self,
        switch_output_key: SwitchOutputKey,
    ) -> impl Stream<Item = bool> {
        self.inner
            .lock()
            .await
            .switch_register(switch_output_key)
            .stream()
            .await
    }
    pub async fn switch_sender(&self, switch_output_key: SwitchOutputKey) -> Sender<bool> {
        self.inner
            .lock()
            .await
            .switch_register(switch_output_key)
            .sender()
    }
}

impl InnerEventRegistry {
    fn new(default_values: Option<ValueSnapshots>) -> Self {
        Self {
            default_values: default_values.unwrap_or_default(),
            clock_registers: Default::default(),
            temperature_registers: Default::default(),
            light_color_registers: Default::default(),
            brightness_color: Default::default(),
            dual_buttons: Default::default(),
            buttons: Default::default(),
            output_switch: Default::default(),
        }
    }
}

#[cfg(test)]
mod test {
    use crate::data::registry::{SwitchOutputKey, ValueSnapshots};

    #[test]
    fn test_serialize_snapshot() {
        let mut snapshots = ValueSnapshots::default();
        snapshots
            .output_switch
            .insert(SwitchOutputKey::Light(Default::default()), true);
        let string = ron::to_string(&snapshots).unwrap();
        println!("{string}");
    }
}
