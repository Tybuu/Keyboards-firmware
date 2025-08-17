use core::{cell::RefCell, future::Future, marker::PhantomData};

use embassy_rp::{
    pio::{Common, Instance, StateMachine},
    pio_programs::ws2812::PioWs2812,
    Peri,
};
use embassy_sync::{
    blocking_mutex::raw::{CriticalSectionRawMutex, ThreadModeRawMutex},
    channel::{Channel, Receiver, Sender, TrySendError},
    mutex::Mutex,
};
use key_lib::{
    keys::{ConfigIndicator, Indicate},
    slave_com::Master,
};
use smart_leds::RGB8;

use crate::slave_com::{HidMaster, HidRequest, HidSlave};

const VAL: u8 = 10;
static CHAN: Channel<CriticalSectionRawMutex, Indicate, 10> = Channel::new();

pub struct MasterIndicatorTask<'d, 'ch, P: Instance, const S: usize> {
    pio: PioWs2812<'d, P, S, 1>,
    hid_chan: HidMaster<'ch>,
    config_num: usize,
    suspended: bool,
    check: bool,
}

impl<'d, 'ch, P: Instance, const S: usize> MasterIndicatorTask<'d, 'ch, P, S> {
    pub fn new(pio: PioWs2812<'d, P, S, 1>, hid_chan: HidMaster<'ch>) -> Self {
        Self {
            pio,
            hid_chan,
            config_num: 0,
            suspended: false,
            check: false,
        }
    }

    async fn indicate_config(&mut self, config_num: usize) {
        match config_num {
            0 => self.pio.write(&[RGB8::new(0, VAL, VAL)]).await,
            1 => self.pio.write(&[RGB8::new(0, 0, VAL)]).await,
            2 => self.pio.write(&[RGB8::new(0, VAL, 0)]).await,
            _ => {}
        }
    }

    pub async fn run(mut self) {
        loop {
            let indicate = CHAN.receive().await;
            match indicate {
                Indicate::Config(config_num) => {
                    if !self.suspended {
                        self.indicate_config(config_num).await;
                        self.hid_chan
                            .send_request(HidRequest::ConfigIndicate(config_num as u8))
                            .await;
                    }
                    self.config_num = config_num;
                }
                Indicate::Enable => {
                    self.suspended = false;
                    self.indicate_config(self.config_num).await;
                }
                Indicate::Disable => {
                    if self.check {
                        self.suspended = true;
                        self.pio.write(&[RGB8::new(0, 0, 0)]).await;
                    } else {
                        self.check = true;
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
pub struct Indicator {}

impl Indicator {
    pub fn suspend(&self, suspended: bool) {
        let msg = if suspended {
            Indicate::Disable
        } else {
            Indicate::Enable
        };
        CHAN.try_send(msg);
    }
}

impl ConfigIndicator for Indicator {
    async fn indicate_config(&self, config_num: Indicate) {
        CHAN.send(config_num).await;
    }
}

pub struct SlaveIndicatorTask<'d, 'ch, P: Instance, const S: usize> {
    pio: PioWs2812<'d, P, S, 1>,
    hid_chan: HidSlave<'ch>,
}

impl<'d, 'ch, P: Instance, const S: usize> SlaveIndicatorTask<'d, 'ch, P, S> {
    pub fn new(pio: PioWs2812<'d, P, S, 1>, hid_chan: HidSlave<'ch>) -> Self {
        Self { pio, hid_chan }
    }

    pub async fn run(mut self) {
        loop {
            let mut req = HidRequest::ConfigIndicate(0);
            self.hid_chan.get_request_ref(&mut req).await;
            if let HidRequest::ConfigIndicate(config_num) = req {
                match config_num {
                    0 => self.pio.write(&[RGB8::new(0, VAL, VAL)]).await,
                    1 => self.pio.write(&[RGB8::new(0, 0, VAL)]).await,
                    2 => self.pio.write(&[RGB8::new(0, VAL, 0)]).await,
                    _ => {}
                }
            }
        }
    }
}
