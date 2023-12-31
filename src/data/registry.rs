use std::hash::Hash;
use std::num::Saturating;
use std::{collections::HashMap, future::Future, ops::DerefMut, sync::Arc, time::Duration};

use crate::data::register::Register;
use chrono::{DateTime, Local, Timelike};
use log::error;
use tokio::{sync::mpsc::Sender, sync::Mutex, time::sleep};
use tokio_stream::Stream;

pub trait TypedKey {
    type Value;
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum ClockKey {
    MinuteClock,
    SecondClock,
}
impl TypedKey for ClockKey {
    type Value = DateTime<Local>;
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum TemperatureKey {
    CurrentTemperature,
    TargetTemperature,
}
impl TypedKey for TemperatureKey {
    type Value = f32;
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum LightColorKey {
    IlluminationColor,
}
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum BrightnessKey {
    IlluminationBrightness,
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct SwitchOutputKey;

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum DualButtonKey {
    DualButton,
}
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum SingleButtonKey {
    SingleButton,
}
#[derive(Copy, Clone, Eq, PartialEq, Hash, Default, Debug)]
pub enum ButtonState<B: Copy + Clone + Eq + Hash> {
    #[default]
    Released,
    ShortPressStart(B),
    LongPressStart(B),
}
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum DualButtonLayout {
    UP,
    DOWN,
}
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct SingleButtonLayout;
pub trait KeyAccess<K: TypedKey<Value = V>, V: Clone + Sync + Send + 'static> {
    async fn register_access<
        'a,
        'b,
        F: Fn(&'a Register<DateTime<Local>>) -> FR + Send + 'a,
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

struct InnerEventRegistry {
    clock_registers: HashMap<ClockKey, Register<DateTime<Local>>>,
    temperature_registers: HashMap<TemperatureKey, Register<f32>>,
    light_color_registers: HashMap<LightColorKey, Register<Saturating<u16>>>,
    brightness_color: HashMap<BrightnessKey, Register<Saturating<u8>>>,
    dual_buttons: HashMap<DualButtonKey, Register<ButtonState<DualButtonLayout>>>,
    buttons: HashMap<SingleButtonKey, Register<ButtonState<SingleButtonLayout>>>,
    output_switch: HashMap<SwitchOutputKey, Register<bool>>,
}

impl InnerEventRegistry {
    fn temperature_register(&mut self, key: TemperatureKey) -> &mut Register<f32> {
        self.temperature_registers
            .entry(key)
            .or_insert_with(|| Register::new(21.0))
    }
    fn light_color_register(&mut self, key: LightColorKey) -> &mut Register<Saturating<u16>> {
        self.light_color_registers
            .entry(key)
            .or_insert_with(|| Register::new(Saturating(200)))
    }
    fn brightness_register(&mut self, key: BrightnessKey) -> &mut Register<Saturating<u8>> {
        self.brightness_color
            .entry(key)
            .or_insert_with(|| Register::new(Saturating(128)))
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
        self.output_switch.entry(key).or_default()
    }
    fn clock(&mut self, clock_key: ClockKey) -> &mut Register<DateTime<Local>> {
        self.clock_registers
            .entry(clock_key)
            .or_insert_with_key(|clock_key| {
                let step_size = match clock_key {
                    ClockKey::MinuteClock => Duration::from_secs(60).as_millis() as u32,
                    ClockKey::SecondClock => Duration::from_secs(1).as_millis() as u32,
                };

                let clock_receiver = Register::new(Local::now());
                let sender = clock_receiver.sender();
                tokio::spawn(async move {
                    loop {
                        let time = Local::now();
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
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(InnerEventRegistry::new())),
        }
    }
    pub async fn clock(&self, key: ClockKey) -> impl Stream<Item = DateTime<Local>> {
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
    fn new() -> Self {
        Self {
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
