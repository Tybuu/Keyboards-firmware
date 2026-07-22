use defmt::info;
use embassy_sync::{blocking_mutex::raw::RawMutex, mutex::Mutex};
use embassy_time::{Duration, Instant};
use heapless::Vec;

use crate::{
    NUM_KEYS,
    descriptor::{KeyboardReportNKRO, MouseReport},
    keys::{ConfigIndicator, Keys},
    position::{KeySensors, KeyState},
    scan_codes::ReportCodes,
};

fn set_bit(num: &mut u8, bit: u8, pos: u8) {
    let mask = 1 << pos;
    if bit == 1 {
        *num |= mask
    } else {
        *num &= !mask
    }
}

fn set_bit_u32(num: u32, bit: u8, pos: u8) -> u32 {
    let mask = 1 << pos;
    if bit == 1 { num | mask } else { num & !mask }
}

enum State {
    Stick(u8),
    Pressed,
    None,
}

#[derive(Copy, Clone, Debug)]
struct MouseDelta {
    initial_press: Option<Instant>,
    next_tick: Instant,
    term0: u64,
    term1: u64,
    check_state: bool,
    res: bool,
}

impl MouseDelta {
    pub fn new(term0: u64, term1: u64) -> Self {
        Self {
            initial_press: None,
            next_tick: Instant::from_micros(0),
            term0,
            term1,
            check_state: false,
            res: false,
        }
    }

    fn reset(&mut self) {
        if !self.check_state {
            self.initial_press = None;
        }
        self.res = false;
        self.check_state = false;
    }

    fn check(&mut self) -> bool {
        if self.check_state {
            self.res
        } else {
            self.update_state();
            self.check_state = true;
            self.res
        }
    }

    fn update_state(&mut self) {
        match self.initial_press {
            Some(time) => {
                let new_time = Instant::now();
                if new_time > self.next_tick {
                    let x = time.elapsed().as_millis();
                    let val = 500000 / (((self.term0 * x.pow(2)) / (x + self.term1)) + 10000);
                    info!("Current val: {}", val);
                    self.next_tick = new_time.checked_add(Duration::from_millis(val)).unwrap();
                    self.res = true;
                } else {
                    self.res = false;
                }
            }
            None => {
                let new_time = Instant::now();
                self.initial_press = Some(new_time);
                self.next_tick = new_time + Duration::from_millis(50);
                self.res = true;
            }
        }
    }
}

pub struct Report {
    key_report: KeyboardReportNKRO,
    mouse_report: MouseReport,
    mouse_delta: MouseDelta,
    scroll_delta: MouseDelta,
    current_layer: usize,
    reset_layer: usize,
    stick: State,
}

impl Report {
    pub fn new() -> Self {
        Self {
            key_report: KeyboardReportNKRO::default(),
            mouse_report: MouseReport::default(),
            mouse_delta: MouseDelta::new(1000000, 500000),
            scroll_delta: MouseDelta::new(1000000, 500000),
            current_layer: 0,
            reset_layer: 0,
            stick: State::None,
        }
    }

    /// Generates a report with the provided keys. Returns a option tuple
    /// where it returns a Some when a report need to be sent
    pub async fn generate_report<I: ConfigIndicator, K: KeyState, M: RawMutex>(
        &mut self,
        keys: &Mutex<M, Keys<I>>,
        positions: &[K; NUM_KEYS],
    ) -> (Option<&KeyboardReportNKRO>, Option<&MouseReport>) {
        let mut new_layer = None;
        let mut pressed_keys = Vec::new();
        let mut new_key_report = KeyboardReportNKRO::default();
        let mut new_mouse_report = MouseReport::default();
        let mut pressed = false;
        let mut stick = false;
        let mut toggle = false;
        keys.lock()
            .await
            .get_keys(self.current_layer, &mut pressed_keys, positions)
            .await;
        for key in pressed_keys {
            match key {
                ReportCodes::Modifier(code) => {
                    let b_idx = code % 8;
                    set_bit(&mut new_key_report.modifier, 1, b_idx);
                }
                ReportCodes::Letter(code) => {
                    let n_idx = (code / 32) as usize;
                    let b_idx = code % 32;
                    match n_idx {
                        0 => new_key_report.nkro_0 = set_bit_u32(new_key_report.nkro_0, 1, b_idx),
                        1 => new_key_report.nkro_1 = set_bit_u32(new_key_report.nkro_1, 1, b_idx),
                        2 => new_key_report.nkro_2 = set_bit_u32(new_key_report.nkro_2, 1, b_idx),
                        3 => new_key_report.nkro_3 = set_bit_u32(new_key_report.nkro_3, 1, b_idx),
                        4 => new_key_report.nkro_4 = set_bit_u32(new_key_report.nkro_4, 1, b_idx),
                        5 => new_key_report.nkro_5 = set_bit_u32(new_key_report.nkro_5, 1, b_idx),
                        6 => new_key_report.nkro_6 = set_bit_u32(new_key_report.nkro_6, 1, b_idx),
                        _ => {}
                    }
                    pressed = true;
                }
                ReportCodes::MouseButton(code) => {
                    let b_idx = code % 8;
                    set_bit(&mut new_mouse_report.buttons, 1, b_idx);
                }
                ReportCodes::MouseX(code) => {
                    if self.mouse_delta.check() {
                        new_mouse_report.x += code;
                    }
                }
                ReportCodes::MouseY(code) => {
                    if self.mouse_delta.check() {
                        new_mouse_report.y += code;
                    }
                }
                ReportCodes::MouseScroll(code) => {
                    if self.scroll_delta.check() {
                        new_mouse_report.wheel += code;
                    }
                }
                ReportCodes::LayerToggle(layer) => {
                    match new_layer {
                        Some(_) => {
                            new_layer = Some(layer);
                        }
                        None => {
                            new_layer = Some(layer);
                        }
                    };
                    toggle = true;
                }
                ReportCodes::Layer(layer) => {
                    if new_layer.is_none() {
                        new_layer = Some(layer);
                    }
                }
                ReportCodes::Sticky => {
                    stick = true;
                }
            };
        }

        self.mouse_delta.reset();
        self.scroll_delta.reset();
        if stick {
            if pressed {
                match self.stick {
                    State::Stick(_) => {
                        self.stick = State::Pressed;
                    }
                    State::Pressed => {}
                    State::None => {
                        self.stick = State::Pressed;
                    }
                }
            } else {
                match self.stick {
                    State::Stick(_) => {
                        if new_key_report.modifier != 0 {
                            self.stick = State::Stick(new_key_report.modifier)
                        }
                    }
                    State::Pressed => {}
                    State::None => {
                        if new_key_report.modifier != 0 {
                            self.stick = State::Stick(new_key_report.modifier)
                        } else {
                            self.stick = State::None;
                        }
                    }
                }
            }
        } else {
            match self.stick {
                State::Stick(val) => {
                    if pressed {
                        new_key_report.modifier = val;
                        self.stick = State::None;
                    }
                }
                State::Pressed => {
                    self.stick = State::None;
                }
                State::None => {}
            }
        }

        match new_layer {
            Some(layer) => {
                if toggle {
                    self.reset_layer = layer as usize;
                }
                self.current_layer = layer as usize;
            }
            None => {
                self.current_layer = self.reset_layer;
            }
        }
        let mut returned_report = (None, None);
        if self.key_report != new_key_report {
            self.key_report = new_key_report;
            returned_report.0 = Some(&self.key_report);
        }

        if self.mouse_report.buttons != new_mouse_report.buttons
            || new_mouse_report.x != 0
            || new_mouse_report.y != 0
            || new_mouse_report.wheel != 0
        {
            self.mouse_report = new_mouse_report;
            returned_report.1 = Some(&self.mouse_report);
        }
        returned_report
    }
}
