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
use key_lib::keys::{ConfigIndicator, Indicate};
use smart_leds::RGB8;

const VAL: u8 = 10;
static CHAN: Channel<CriticalSectionRawMutex, Indicate, 10> = Channel::new();

pub struct IndicatorTask<'d, P: Instance, const S: usize> {
    pio: PioWs2812<'d, P, S, 1>,
    config_num: usize,
    suspended: bool,
    check: bool,
}

impl<'d, P: Instance, const S: usize> IndicatorTask<'d, P, S> {
    pub fn new(pio: PioWs2812<'d, P, S, 1>) -> Self {
        Self {
            pio,
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
