use core::{future::Future, marker::PhantomData, ops::Range};

use embassy_futures::select::{select4, select_array, select_slice};
use embassy_nrf::{
    gpio::{AnyPin, Input, Output},
    gpiote::{AnyChannel, InputChannel},
};
use embassy_time::{Duration, Instant};
use heapless::Vec;
use key_lib::{position::KeySensors, NUM_KEYS};

use crate::radio::{receive_channel, Packet, Radio};

const DEBOUNCE_TIME: u64 = 5;
#[derive(Copy, Clone, Debug)]
struct Debouncer {
    state: bool,
    debounced: Option<Instant>,
}

impl Debouncer {
    const fn default() -> Debouncer {
        Self {
            state: false,
            debounced: None,
        }
    }
    /// Returns the pressed status of the position
    fn is_pressed(&self) -> bool {
        self.state
    }

    /// Updates the buf of the key. Updating the buf will also update
    /// the value returned from the is_pressed function
    fn update_buf(&mut self, buf: bool) {
        match self.debounced {
            Some(time) => {
                if time.elapsed() > Duration::from_millis(DEBOUNCE_TIME) {
                    self.state = buf;
                    self.debounced = None;
                }
            }
            None => {
                if buf != self.state {
                    self.debounced = Some(Instant::now());
                }
            }
        }
    }
}

pub struct Matrix<'a, const INPUT_SIZE: usize, const OUTPUT_SIZE: usize> {
    out: [Output<'a>; OUTPUT_SIZE],
    input: [Input<'a>; INPUT_SIZE],
    valid_input: [[bool; OUTPUT_SIZE]; INPUT_SIZE],
    debouncers: [[Debouncer; OUTPUT_SIZE]; INPUT_SIZE],
    pressed: Option<Instant>,
}

impl<'a, const INPUT_SIZE: usize, const OUTPUT_SIZE: usize> Matrix<'a, INPUT_SIZE, OUTPUT_SIZE> {
    pub fn disable_debouncer(&mut self, range: Range<usize>) {
        let res = self.valid_input.iter_mut().flatten().skip(range.start);
        for input in res.take(range.len()) {
            *input = false;
        }
    }
    pub fn new(out: [Output<'a>; OUTPUT_SIZE], input: [Input<'a>; INPUT_SIZE]) -> Self {
        Self {
            out,
            input,
            valid_input: [[true; OUTPUT_SIZE]; INPUT_SIZE],
            debouncers: [[Debouncer::default(); OUTPUT_SIZE]; INPUT_SIZE],
            pressed: None,
        }
    }

    pub async fn update(&mut self) {
        // If no keys were pressed in the previous scan,
        // we'll set all the output pins high and await
        // for one of the channels to go high to save battery
        if let Some(time) = self.pressed {
            if time.elapsed() >= Duration::from_millis(DEBOUNCE_TIME) {
                for power in &mut self.out {
                    power.set_high();
                }

                // let mut high = false;
                // for row in &mut self.input {
                //     high = high || row.is_high()
                // }

                let futures: Vec<_, INPUT_SIZE> = self
                    .input
                    .iter_mut()
                    .map(|pin| pin.wait_for_high())
                    .collect();
                unsafe {
                    select_array(futures.into_array::<INPUT_SIZE>().unwrap_unchecked()).await;
                }

                for power in &mut self.out {
                    power.set_low();
                }
            }
        }

        let mut pressed = false;
        for i in 0..OUTPUT_SIZE {
            self.out[i].set_high();
            for j in 0..INPUT_SIZE {
                self.debouncers[j][i].update_buf(self.input[j].is_high());
                pressed = pressed || self.debouncers[j][i].is_pressed();
            }
            self.out[i].set_low();
        }
        if pressed {
            self.pressed = None;
        } else {
            match self.pressed {
                Some(_) => {}
                None => {
                    self.pressed = Some(Instant::now());
                }
            }
        }
    }

    pub fn get_state(&self) -> u32 {
        let mut index = 0;
        let mut state = 0u32;
        self.debouncers
            .iter()
            .flatten()
            .zip(self.valid_input.iter().flatten())
            .for_each(|(deb, valid)| {
                if *valid {
                    if deb.is_pressed() {
                        state |= 1 << index;
                    }
                    index += 1;
                }
            });
        state
    }
}

pub struct DongleSensors {
    // rad: Radio<'d>,
}

impl KeySensors for DongleSensors {
    type Item = bool;

    async fn update_positions<K: key_lib::position::KeyState<Item = Self::Item>>(
        &mut self,
        positions: &mut [K],
    ) {
        const OFFSET: usize = NUM_KEYS / 2;
        let (addr, key_states) = receive_channel().await;
        if addr == 1 {
            positions[..OFFSET]
                .iter_mut()
                .enumerate()
                .for_each(|(i, k)| {
                    let state = (key_states >> i) & 1 != 0;
                    k.update_buf(state);
                });
        } else if addr == 2 {
            positions[OFFSET..]
                .iter_mut()
                .enumerate()
                .for_each(|(i, k)| {
                    let state = (key_states >> i) & 1 != 0;
                    k.update_buf(state);
                });
        }
    }
}
