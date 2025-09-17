use embassy_nrf::interrupt::{self, typelevel};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, waitqueue::AtomicWaker,
};

use crate::{
    radio::packet::Packet, DONGLE_ADDRESS, DONGLE_PREFIX, KEYBOARD_ADDRESS, LEFT_PREFIX,
    RIGHT_PREFIX,
};

mod inner_radio;
pub mod packet;
pub mod radio;
pub mod simple;

pub(in super::radio) static STATE: AtomicWaker = AtomicWaker::new();

#[derive(Clone, Copy)]
pub struct Addresses {
    pub base: [u32; 2],
    pub prefix: [[u8; 4]; 2],
}

impl Default for Addresses {
    fn default() -> Self {
        let mut res = Self {
            base: Default::default(),
            prefix: Default::default(),
        };
        res.base[0] = DONGLE_ADDRESS;
        res.base[1] = KEYBOARD_ADDRESS;
        res.prefix[0][0] = DONGLE_PREFIX;
        res.prefix[0][1] = LEFT_PREFIX;
        res.prefix[0][2] = RIGHT_PREFIX;
        res
    }
}

pub struct InterruptHandler {}

impl interrupt::typelevel::Handler<typelevel::RADIO> for InterruptHandler {
    unsafe fn on_interrupt() {
        let r = embassy_nrf::pac::RADIO;
        r.intenclr().write(|w| w.0 = 0xFFFF_FFFF);
        STATE.wake();
    }
}

const NUM_PACKETS: usize = 5;
pub(in super::radio) static RECV_CHANNEL: Channel<CriticalSectionRawMutex, Packet, NUM_PACKETS> =
    Channel::new();
pub(in super::radio) static SEND_CHANNEL: Channel<CriticalSectionRawMutex, Packet, NUM_PACKETS> =
    Channel::new();

pub async fn send_packet(packet: &Packet) {
    SEND_CHANNEL.send(*packet).await;
}
pub async fn receive_packet() -> Packet {
    RECV_CHANNEL.receive().await
}
