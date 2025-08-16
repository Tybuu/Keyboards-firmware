use core::{mem, ops::Range};

use defmt::{error, info};
use embassy_time::Timer;
use embassy_usb::driver::Driver;
use heapless::Vec;
use sequential_storage::map::Value;

use crate::{
    NUM_KEYS, NUM_LAYERS,
    codes::{HidScanCodeType, MAX_SERIAL_LENGTH, ScanCodeBehavior, ScanCodeLayerStorage},
    com::{ContinuousReader, ContinuousWriter},
    position::{KeySensors, KeyState},
    scan_codes::ReportCodes,
    slave_com::{Slave, SlaveState},
    storage::{StorageItem, StorageKey, get_item, store_val},
};

pub enum Indicate {
    Config(usize),
    Enable,
    Disable,
}
pub trait ConfigIndicator {
    fn indicate_config(&self, config_num: Indicate) -> impl Future<Output = ()>;
}

enum PressResult {
    Pressed,
    Function,
    None,
}

#[derive(Copy, Clone, Debug)]
pub struct Keys<K: KeyState, I: ConfigIndicator> {
    codes: [[ScanCodeBehavior; NUM_LAYERS]; NUM_KEYS],
    key_states: [K; NUM_KEYS],
    indicator: Option<I>,
    pub current_layer: [Option<usize>; NUM_KEYS],
    pub config_num: usize,
}

impl<K: KeyState, I: ConfigIndicator> Keys<K, I> {
    /// Returns a Keys struct
    pub const fn default() -> Self {
        Self {
            codes: [[ScanCodeBehavior::default(); NUM_LAYERS]; NUM_KEYS],
            key_states: [K::DEFAULT; NUM_KEYS],
            indicator: None,
            current_layer: [None; NUM_KEYS],
            config_num: 0,
        }
    }

    pub fn set_indicator(&mut self, indicator: I) {
        self.indicator = Some(indicator);
    }

    pub fn set_position_type_ranged(&mut self, range: Range<usize>, switch_type: K) {
        self.key_states[range].fill(switch_type);
    }

    pub fn get_pressed(&self, index: usize) -> bool {
        self.key_states[index].is_pressed()
    }

    pub fn set_code(&mut self, code: ScanCodeBehavior, index: usize, layer: usize) {
        self.codes[index][layer] = code;
    }

    pub async fn update_positions(&mut self, sensors: &mut impl KeySensors<Item = K::Item>) {
        sensors.update_positions(&mut self.key_states).await;
    }

    #[cfg(feature = "hall-effect")]
    pub async fn setup_positions(&mut self, sensors: &mut impl KeySensors<Item = K::Item>) {
        sensors.setup(&mut self.key_states).await;
    }

    /// Returns the indexes of all the keys that are pressed to the vec
    pub fn is_pressed(&self, vec: &mut Vec<usize, NUM_KEYS>) {
        vec.extend(
            self.key_states
                .iter()
                .enumerate()
                .filter_map(|(i, pos)| if pos.is_pressed() { Some(i) } else { None }),
        );
    }

    /// Pushes the resulting ScanResult onto the provided vec depending on the indexed key's
    /// position. Returns true if a key was pushed into the provided index set
    async fn get_pressed_code(
        &mut self,
        index: usize,
        layer: usize,
        set: &mut Vec<ReportCodes, 64>,
    ) -> PressResult {
        let pressed = self.key_states[index].is_pressed();
        match self.codes[index][layer] {
            ScanCodeBehavior::Single(code) => {
                if pressed {
                    set.push(code.into()).unwrap();
                    PressResult::Pressed
                } else {
                    PressResult::None
                }
            }
            ScanCodeBehavior::Double(code0, code1) => {
                if pressed {
                    set.push(code0.into()).unwrap();
                    set.push(code1.into()).unwrap();
                    PressResult::Pressed
                } else {
                    PressResult::None
                }
            }
            ScanCodeBehavior::Triple(code0, code1, code2) => {
                if pressed {
                    set.push(code0.into()).unwrap();
                    set.push(code1.into()).unwrap();
                    set.push(code2.into()).unwrap();
                    PressResult::Pressed
                } else {
                    PressResult::None
                }
            }
            ScanCodeBehavior::CombinedKey {
                other_index,
                normal_code,
                combined_code: other_key_code,
            } => {
                if pressed {
                    set.push(ReportCodes::Sticky).unwrap();
                    if self.key_states[other_index].is_pressed() {
                        set.push(other_key_code.into()).unwrap();
                        PressResult::Pressed
                    } else {
                        set.push(normal_code.into()).unwrap();
                        PressResult::Pressed
                    }
                } else {
                    PressResult::None
                }
            }
            ScanCodeBehavior::ChangeConfig(config_num) => {
                if pressed {
                    self.load_keys_from_storage(config_num as usize).await;
                    PressResult::Function
                } else {
                    PressResult::None
                }
            }
        }
    }

    /// Returns all the pressed scancodes in the Keys struct. Returns it through
    /// the passed in vector. The passed in vector should be empty.
    /// Note that if a key is held, it will ignore the passed in layer and use the
    /// previous layer it's holding
    pub async fn get_keys(&mut self, layer: usize, set: &mut Vec<ReportCodes, 64>) {
        for i in 0..NUM_KEYS {
            let layer = match self.current_layer[i] {
                Some(num) => num,
                None => layer,
            };
            match self.get_pressed_code(i, layer, set).await {
                PressResult::Function => {
                    set.clear();
                    self.key_states.iter_mut().for_each(|s| s.reset());
                    self.current_layer.fill(None);
                    // Slight delay so user can have time to release the key activating the
                    // function so the function doesn't activate again
                    Timer::after_millis(500).await;
                    break;
                }
                PressResult::Pressed => {
                    self.current_layer[i] = Some(layer);
                }
                PressResult::None => {
                    self.current_layer[i] = None;
                }
            }
        }
    }

    pub async fn write_keys_to_com<'d, T: Driver<'d>>(&self, writer: &mut ContinuousWriter<'d, T>) {
        let mut buf = [0u8; MAX_SERIAL_LENGTH];
        for codes in self.codes {
            for code in codes {
                code.into_buffer(&mut buf[..code.into_buffer_len()])
                    .unwrap();
                writer.write(&buf[..code.into_buffer_len()]).await;
            }
        }
    }

    pub async fn write_keys_to_storage(&self, config_num: usize) {
        for layer in 0..NUM_LAYERS {
            let new_keys = StorageItem::Key(ScanCodeLayerStorage {
                codes: self.codes.map(|codes| codes[layer]),
            });
            let StorageItem::Key(keys) = &new_keys;
            let storage_key = StorageKey::KeyScanCode { config_num, layer };
            let stored_keys = get_item(storage_key).await;
            match stored_keys {
                Some(stored_keys) => {
                    if let StorageItem::Key(stored_keys) = stored_keys {
                        if stored_keys != *keys {
                            info!("Storing config {} | layer {}", config_num, layer);
                            store_val(storage_key, &new_keys).await;
                        } else {
                            info!("Equal config {} | layer {}", config_num, layer);
                        }
                    } else {
                    }
                }
                None => {
                    info!("No config {} | layer {}", config_num, layer);
                    store_val(storage_key, &new_keys).await;
                }
            }
        }
    }

    pub async fn load_keys_from_storage(&mut self, config_num: usize) -> Result<(), ()> {
        self.config_num = config_num;
        for layer in 0..NUM_LAYERS {
            let storage_key = StorageKey::KeyScanCode { config_num, layer };
            match get_item(storage_key).await {
                Some(val) => match val {
                    StorageItem::Key(codes) => {
                        self.codes
                            .iter_mut()
                            .zip(codes.codes.iter())
                            .for_each(|(key, code)| key[layer] = *code);
                    }
                    _ => {
                        error!("Invalid key stored at {}", storage_key);
                        return Err(());
                    }
                },
                None => {
                    *self = Keys::default();
                    error!("No key stored at {}", storage_key);
                    return Err(());
                }
            }
        }
        if let Some(indicator) = self.indicator.as_ref() {
            indicator
                .indicate_config(Indicate::Config(self.config_num))
                .await;
        }
        Ok(())
    }
    pub async fn load_keys_from_com<'d, T: Driver<'d>>(
        &mut self,
        reader: &mut ContinuousReader<'d, T>,
        config_num: usize,
    ) -> Result<(), sequential_storage::map::SerializationError> {
        self.config_num = config_num;
        let mut buf = [0u8; MAX_SERIAL_LENGTH];
        for code in self.codes.iter_mut().flatten() {
            buf[0] = reader.pop().await;
            let hid_type: HidScanCodeType = buf[0]
                .try_into()
                .map_err(|_| sequential_storage::map::SerializationError::InvalidFormat)?;
            reader.pop_slice(&mut buf[1..hid_type.get_len()]).await;
            *code = ScanCodeBehavior::deserialize_from(&buf[..hid_type.get_len()]).unwrap();
        }
        if let Some(indicator) = self.indicator.as_ref() {
            indicator
                .indicate_config(Indicate::Config(self.config_num))
                .await;
        }
        Ok(())
    }
}

pub struct SlaveKeys<K: KeyState<Item = KS::Item>, KS: KeySensors, SL: SlaveState, S: Slave> {
    states: [K; NUM_KEYS / 2],
    sensors: KS,
    slave_state: SL,
    slave_sender: S,
}

impl<K: KeyState<Item = KS::Item>, KS: KeySensors, SL: SlaveState, S: Slave<SlaveState = SL>>
    SlaveKeys<K, KS, SL, S>
{
    pub fn new(sensors: KS, slave_sender: S) -> Self {
        Self {
            states: [K::DEFAULT; NUM_KEYS / 2],
            sensors,
            slave_state: SL::DEFAULT,
            slave_sender,
        }
    }

    #[cfg(feature = "hall-effect")]
    pub async fn setup_keys(&mut self) {
        self.sensors.setup(&mut self.states).await;
    }

    pub async fn send_report(&mut self) {
        self.sensors.update_positions(&mut self.states).await;
        let mut new_state = SL::DEFAULT;
        for (i, state) in self.states.iter().enumerate() {
            new_state.update_state(i, state.is_pressed());
        }
        if new_state != self.slave_state {
            self.slave_state = new_state;
            self.slave_sender.send_slave_state(self.slave_state).await;
        }
    }
}
