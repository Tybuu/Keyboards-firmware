use embassy_rp::{
    adc::{Adc, Async, Channel},
    gpio::Output,
};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, channel::Receiver};
use embassy_time::Timer;

use key_lib::{
    position::{KeySensors, KeyState},
    NUM_KEYS,
};

pub struct HallEffectSensors<'p, 'd, const N: usize, const M: usize> {
    chans: [Channel<'p>; N],
    sel: [Output<'p>; M],
    adc: Adc<'d, Async>,
    order: [usize; NUM_KEYS / 2],
}

impl<'p, 'd, const N: usize, const M: usize> HallEffectSensors<'p, 'd, N, M> {
    pub fn new(
        chans: [Channel<'p>; N],
        sel: [Output<'p>; M],
        adc: Adc<'d, Async>,
        order: [usize; NUM_KEYS / 2],
    ) -> Self {
        Self {
            chans,
            sel,
            adc,
            order,
        }
    }
}

fn change_sel<'p>(pins: &mut [Output<'p>], sel: usize) {
    // For each pin, bit shift the sel with the respective index and mask that value to determine
    // if the pin should be high or low
    pins.iter_mut().enumerate().for_each(|(i, pin)| {
        if ((sel >> i) & 1) == 1 {
            pin.set_high();
        } else {
            pin.set_low();
        }
    });
}

impl<'p, 'd, const N: usize, const M: usize> KeySensors for HallEffectSensors<'p, 'd, N, M> {
    type Item = u16;
    async fn update_positions<T: KeyState<Item = Self::Item>>(&mut self, positions: &mut [T]) {
        for (i, &pos) in self.order.iter().enumerate() {
            let chan = i % self.chans.len();
            if chan == 0 {
                let sel = i / self.chans.len();
                change_sel(&mut self.sel, sel);
                Timer::after_micros(1).await;
            }
            positions[pos].update_buf(self.adc.read(&mut self.chans[chan]).await.unwrap());
        }
    }

    async fn setup<K: KeyState<Item = Self::Item>>(&mut self, positions: &mut [K]) {
        let mut setup = false;
        while !setup {
            setup = true;
            for (i, &pos) in self.order.iter().enumerate() {
                let chan = i % self.chans.len();
                if chan == 0 {
                    let sel = i / self.chans.len();
                    change_sel(&mut self.sel, sel);
                }
                let res = positions[pos].setup(self.adc.read(&mut self.chans[chan]).await.unwrap());
                // If any key isn't setup, the && will cause setup to be false leading to setup
                // being false after the loop
                setup = setup && res;
            }
        }
    }
}

pub struct MasterSensors<'p, 'd, 'ch, const N: usize, const M: usize> {
    sensors: HallEffectSensors<'p, 'd, N, M>,
    slave_chan: Receiver<'ch, ThreadModeRawMutex, u32, 5>,
}

impl<'p, 'd, 'ch, const N: usize, const M: usize> MasterSensors<'p, 'd, 'ch, N, M> {
    pub fn new(
        chans: [Channel<'p>; N],
        sel: [Output<'p>; M],
        adc: Adc<'d, Async>,
        slave_chan: Receiver<'ch, ThreadModeRawMutex, u32, 5>,
        order: [usize; NUM_KEYS / 2],
    ) -> Self {
        Self {
            sensors: HallEffectSensors::new(chans, sel, adc, order),
            slave_chan,
        }
    }
}

impl<'p, 'd, 'ch, const N: usize, const M: usize> KeySensors for MasterSensors<'p, 'd, 'ch, N, M> {
    type Item = u16;
    async fn update_positions<T: KeyState<Item = Self::Item>>(&mut self, positions: &mut [T]) {
        self.sensors.update_positions(positions).await;
        if let Ok(slave_rep) = self.slave_chan.try_receive() {
            let offset = NUM_KEYS / 2;
            for i in 0..(offset) {
                let val = (slave_rep >> i) & 1;
                positions[i + offset].update_buf(val as u16);
            }
        }
    }

    async fn setup<K: KeyState<Item = Self::Item>>(&mut self, positions: &mut [K]) {
        self.sensors.setup(positions).await;
    }
}
