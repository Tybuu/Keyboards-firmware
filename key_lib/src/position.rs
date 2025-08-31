#[cfg(feature = "hall-effect")]
pub const DEFAULT_HIGH: u32 = 1700;
#[cfg(feature = "hall-effect")]
pub const DEFAULT_LOW: u32 = 1400;
#[cfg(feature = "hall-effect")]
const DIF: f32 = (DEFAULT_HIGH - DEFAULT_LOW) as f32;
#[cfg(feature = "hall-effect")]
const DEFAULT_RELEASE_SCALE: f32 = 0.30;
#[cfg(feature = "hall-effect")]
const DEFAULT_ACTUATE_SCALE: f32 = 0.35;
#[cfg(feature = "hall-effect")]
const TOLERANCE_SCALE: f32 = 0.1;
#[cfg(feature = "hall-effect")]
const BUFFER_SIZE: usize = 1;

pub trait KeyState: Copy {
    const DEFAULT: Self;
    type Item;
    fn update_buf(&mut self, buf: Self::Item);

    fn is_pressed(&self) -> bool;

    fn reset(&mut self);

    #[cfg(feature = "hall-effect")]
    fn is_analog(&self) -> bool;

    #[cfg(feature = "hall-effect")]
    fn get_buf(&self) -> Self::Item;

    #[cfg(feature = "hall-effect")]
    fn calibrate(&mut self, buf: Self::Item);

    #[cfg(feature = "hall-effect")]
    fn setup(&mut self, buf: Self::Item) -> bool;
}

#[derive(Copy, Clone, Debug)]
pub struct DefaultSwitch {
    state: bool,
}

impl KeyState for DefaultSwitch {
    const DEFAULT: Self = Self { state: false };
    type Item = bool;
    fn update_buf(&mut self, buf: Self::Item) {
        self.state = buf;
    }

    fn is_pressed(&self) -> bool {
        self.state
    }

    fn reset(&mut self) {
        self.state = false;
    }

    #[cfg(feature = "hall-effect")]
    fn is_analog(&self) -> bool {
        false
    }

    #[cfg(feature = "hall-effect")]
    fn calibrate(&mut self, _: Self::Item) {}

    #[cfg(feature = "hall-effect")]
    fn get_buf(&self) -> Self::Item {
        self.state
    }

    #[cfg(feature = "hall-effect")]
    fn setup(&mut self, _: Self::Item) -> bool {
        true
    }
}

// Makes hall effect switches act like a normal mechanical switch
#[cfg(feature = "hall-effect")]
#[derive(Copy, Clone, Default, Debug)]
pub struct DigitalPosition {
    buffer: [u16; BUFFER_SIZE], // Take multiple readings to smooth out buffer
    buffer_pos: usize,
    release_point: u16,
    actuation_point: u16,
    lowest_point: u16,
    highest_point: u16,
    pressed: bool,
}

#[cfg(feature = "hall-effect")]
impl KeyState for DigitalPosition {
    type Item = u16;
    const DEFAULT: Self = Self {
        buffer: [0; BUFFER_SIZE],
        buffer_pos: 0,
        release_point: (DEFAULT_HIGH - (DEFAULT_RELEASE_SCALE * DIF) as u32) as u16,
        actuation_point: (DEFAULT_HIGH - (DEFAULT_ACTUATE_SCALE * DIF) as u32) as u16,
        pressed: false,
        lowest_point: DEFAULT_LOW as u16,
        highest_point: DEFAULT_HIGH as u16,
    };

    // is_pressed is set like a normal mechanical switch, where if the buf
    // is higher than the release point, is_pressed is false, and if
    // the buf is lower than the acutation point, is_pressed is true
    fn update_buf(&mut self, pos: u16) {
        self.buffer[self.buffer_pos] = pos;
        self.buffer_pos = (self.buffer_pos + 1) % BUFFER_SIZE;
        let mut sum = 0;
        for buf in self.buffer {
            sum += buf;
        }
        let avg = sum / BUFFER_SIZE as u16;
        self.calibrate(avg);
        if avg <= self.actuation_point {
            self.pressed = true;
        } else if avg > self.release_point {
            self.pressed = false;
        }
    }

    fn is_pressed(&self) -> bool {
        self.pressed
    }

    fn get_buf(&self) -> u16 {
        let mut sum = 0;
        for buf in self.buffer {
            sum += buf;
        }
        sum / BUFFER_SIZE as u16
    }

    // Keep calling this function with adc readings
    // until it returns true to calibrate keys
    fn setup(&mut self, reading: u16) -> bool {
        if self.buffer[0] == 0 || self.buffer_pos != 0 {
            self.buffer[self.buffer_pos] = reading;
            self.buffer_pos = (self.buffer_pos + 1) % BUFFER_SIZE;
            false
        } else {
            let mut buf = 0;
            for num in self.buffer {
                buf += num;
            }
            let avg = buf / BUFFER_SIZE as u16;
            self.calibrate(avg);
            true
        }
    }

    fn calibrate(&mut self, buf: u16) {
        let mut changed = false;
        if self.highest_point < buf {
            self.highest_point = buf;
            changed = true;
        } else if self.lowest_point > buf {
            self.lowest_point = buf;
            changed = true;
        }

        if changed {
            let dif = (self.highest_point - self.lowest_point) as f32;
            self.release_point = self.highest_point - (DEFAULT_RELEASE_SCALE * dif) as u16;
            self.actuation_point = self.highest_point - (DEFAULT_ACTUATE_SCALE * dif) as u16;
        }
    }

    fn is_analog(&self) -> bool {
        true
    }

    fn reset(&mut self) {
        self.buffer.fill(self.highest_point);
        self.buffer_pos = 0;
        self.pressed = false;
    }
}

#[derive(Copy, Clone, Default, Debug)]
#[cfg(feature = "hall-effect")]
pub struct WootingPosition {
    buffer: [u16; BUFFER_SIZE], // Take multiple readings to smooth out buffer
    buffer_pos: usize,
    release_point: u16,
    actuation_point: u16,
    lowest_point: u16,
    highest_point: u16,
    pressed: bool,
    last_pos: u16,
    wooting: bool,
    tolerance: u16,
}

#[cfg(feature = "hall-effect")]
impl KeyState for WootingPosition {
    type Item = u16;
    const DEFAULT: Self = Self {
        buffer: [0; BUFFER_SIZE],
        last_pos: 0,
        buffer_pos: 0,
        release_point: (DEFAULT_HIGH - (DEFAULT_RELEASE_SCALE * DIF) as u32) as u16,
        actuation_point: (DEFAULT_HIGH - (DEFAULT_ACTUATE_SCALE * DIF) as u32) as u16,
        lowest_point: DEFAULT_LOW as u16,
        highest_point: DEFAULT_HIGH as u16,
        pressed: false,
        wooting: false,
        tolerance: (DIF * TOLERANCE_SCALE) as u16,
    };

    fn update_buf(&mut self, pos: u16) {
        self.buffer[self.buffer_pos] = pos;
        self.buffer_pos = (self.buffer_pos + 1) % BUFFER_SIZE;
        let mut sum = 0;
        for buf in self.buffer {
            sum += buf;
        }
        let avg = sum / BUFFER_SIZE as u16;
        if avg > self.release_point {
            self.last_pos = avg;
            self.wooting = false;
            self.pressed = false;
            self.calibrate(avg);
        } else if avg < self.lowest_point {
            self.last_pos = avg;
            self.wooting = true;
            self.pressed = true;
            self.calibrate(avg);
        } else if avg < self.last_pos - self.tolerance
            || (avg <= self.actuation_point && !self.wooting)
        {
            self.last_pos = avg;
            self.wooting = true;
            self.pressed = true;
        } else if avg > self.last_pos + self.tolerance {
            self.last_pos = avg;
            self.pressed = false;
        }
    }

    fn calibrate(&mut self, buf: u16) {
        let mut changed = false;
        if self.highest_point < buf {
            self.highest_point = buf;
            changed = true;
        } else if self.lowest_point > buf {
            self.lowest_point = buf;
            changed = true;
        }

        if changed {
            let dif = (self.highest_point - self.lowest_point) as f32;
            self.release_point = self.highest_point - (DEFAULT_RELEASE_SCALE * dif) as u16;
            self.actuation_point = self.highest_point - (DEFAULT_ACTUATE_SCALE * dif) as u16;
            self.tolerance = (dif * TOLERANCE_SCALE) as u16;
        }
    }

    fn setup(&mut self, reading: u16) -> bool {
        if self.buffer[0] == 0 || self.buffer_pos != 0 {
            self.buffer[self.buffer_pos] = reading;
            self.buffer_pos = (self.buffer_pos + 1) % BUFFER_SIZE;
            false
        } else {
            let mut buf = 0;
            for num in self.buffer {
                buf += num;
            }
            let avg = buf / BUFFER_SIZE as u16;
            self.calibrate(avg);
            true
        }
    }

    fn is_pressed(&self) -> bool {
        self.pressed
    }

    fn get_buf(&self) -> u16 {
        let mut sum = 0;
        for buf in self.buffer {
            sum += buf;
        }
        sum / BUFFER_SIZE as u16
    }

    fn is_analog(&self) -> bool {
        true
    }

    fn reset(&mut self) {
        self.buffer.fill(self.highest_point);
        self.pressed = false;
        self.wooting = false;
        self.buffer_pos = 0;
    }
}

#[derive(Copy, Clone)]
#[cfg(feature = "hall-effect")]
pub struct SlavePosition {
    state: u16,
    analog_reading: u16,
}

#[cfg(feature = "hall-effect")]
impl KeyState for SlavePosition {
    const DEFAULT: Self = Self {
        state: 0,
        analog_reading: u16::MAX,
    };
    type Item = u16;

    fn update_buf(&mut self, buf: Self::Item) {
        if buf > 1 {
            self.analog_reading = buf;
        } else {
            self.state = buf;
        }
    }

    fn get_buf(&self) -> Self::Item {
        self.analog_reading
    }

    fn is_pressed(&self) -> bool {
        self.state != 0
    }

    fn is_analog(&self) -> bool {
        true
    }

    fn reset(&mut self) {
        self.state = 0;
        self.analog_reading = u16::MAX;
    }

    fn calibrate(&mut self, _: Self::Item) {}

    fn setup(&mut self, _: Self::Item) -> bool {
        true
    }
}

#[derive(Copy, Clone)]
#[cfg(feature = "hall-effect")]
pub enum HeSwitch {
    Wooting(WootingPosition),
    Digital(DigitalPosition),
    Slave(SlavePosition),
}

#[cfg(feature = "hall-effect")]
impl KeyState for HeSwitch {
    const DEFAULT: Self = { Self::Wooting(WootingPosition::DEFAULT) };

    type Item = u16;

    fn update_buf(&mut self, buf: Self::Item) {
        match self {
            HeSwitch::Wooting(wp) => wp.update_buf(buf),
            HeSwitch::Digital(dp) => dp.update_buf(buf),
            HeSwitch::Slave(sp) => sp.update_buf(buf),
        }
    }

    fn get_buf(&self) -> Self::Item {
        match self {
            HeSwitch::Wooting(wp) => wp.get_buf(),
            HeSwitch::Digital(dp) => dp.get_buf(),
            HeSwitch::Slave(sp) => sp.get_buf(),
        }
    }

    fn is_pressed(&self) -> bool {
        match self {
            HeSwitch::Wooting(wp) => wp.is_pressed(),
            HeSwitch::Digital(dp) => dp.is_pressed(),
            HeSwitch::Slave(sp) => sp.is_pressed(),
        }
    }

    fn is_analog(&self) -> bool {
        true
    }

    fn reset(&mut self) {
        match self {
            HeSwitch::Wooting(wp) => wp.reset(),
            HeSwitch::Digital(dp) => dp.reset(),
            HeSwitch::Slave(sp) => sp.reset(),
        }
    }

    fn calibrate(&mut self, buf: Self::Item) {
        match self {
            HeSwitch::Wooting(wp) => wp.calibrate(buf),
            HeSwitch::Digital(dp) => dp.calibrate(buf),
            HeSwitch::Slave(sp) => sp.calibrate(buf),
        }
    }

    fn setup(&mut self, buf: Self::Item) -> bool {
        match self {
            HeSwitch::Wooting(wp) => wp.setup(buf),
            HeSwitch::Digital(dp) => dp.setup(buf),
            HeSwitch::Slave(sp) => sp.setup(buf),
        }
    }
}

pub trait KeySensors {
    type Item;
    fn update_positions<K: KeyState<Item = Self::Item>>(
        &mut self,
        positions: &mut [K],
    ) -> impl core::future::Future<Output = ()>;

    #[cfg(feature = "hall-effect")]
    fn setup<K: KeyState<Item = Self::Item>>(
        &mut self,
        positions: &mut [K],
    ) -> impl core::future::Future<Output = ()>;
}
