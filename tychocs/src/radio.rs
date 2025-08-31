use core::{sync::atomic::compiler_fence, task::Poll};

use defmt::info;
use embassy_nrf::{
    interrupt::{
        self,
        typelevel::{self, Interrupt},
    },
    pac::radio::regs::{Rxaddresses, Txaddress},
    Peri,
};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, waitqueue::AtomicWaker,
};

use crate::{DONGLE_ADDRESS, DONGLE_PREFIX, KEYBOARD_ADDRESS, LEFT_PREFIX, RIGHT_PREFIX};

const BUFFER_SIZE: usize = 32;
const META_SIZE: usize = 2;

static STATE: AtomicWaker = AtomicWaker::new();
static CHANNEL: Channel<CriticalSectionRawMutex, (u8, u32), 10> = Channel::new();

pub struct InterruptHandler {}

impl interrupt::typelevel::Handler<typelevel::RADIO> for InterruptHandler {
    unsafe fn on_interrupt() {
        let r = embassy_nrf::pac::RADIO;
        r.intenclr().write(|w| w.0 = 0xFFFF_FFFF);
        STATE.wake();
    }
}

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

pub struct Radio<'d> {
    _radio: Peri<'d, embassy_nrf::peripherals::RADIO>,
    id: [u8; 2],
}

impl<'d> Radio<'d> {
    pub fn new(
        _radio: Peri<'d, embassy_nrf::peripherals::RADIO>,
        _irq: impl interrupt::typelevel::Binding<
            embassy_nrf::interrupt::typelevel::RADIO,
            InterruptHandler,
        >,
        addresses: Addresses,
    ) -> Self {
        let r = embassy_nrf::pac::RADIO;

        r.power().write(|w| w.set_power(false));
        r.power().write(|w| w.set_power(true));

        r.mode()
            .write(|w| w.set_mode(embassy_nrf::pac::radio::vals::Mode::NRF_2MBIT));

        r.pcnf0().write(|w| {
            w.set_lflen(8);
            w.set_s0len(false);
            w.set_s1len(1);
            w.set_s1incl(embassy_nrf::pac::radio::vals::S1incl::AUTOMATIC);
            w.set_plen(embassy_nrf::pac::radio::vals::Plen::_16BIT);
        });

        r.pcnf1().write(|w| {
            w.set_maxlen(BUFFER_SIZE as u8);
            w.set_statlen(0);
            w.set_balen(4);
            w.set_endian(embassy_nrf::pac::radio::vals::Endian::LITTLE);
        });

        r.base0().write_value(addresses.base[0]);
        r.base1().write_value(addresses.base[1]);
        r.prefix0()
            .write(|w| w.0 = u32::from_le_bytes(addresses.prefix[0]));
        r.prefix1()
            .write(|w| w.0 = u32::from_le_bytes(addresses.prefix[1]));

        r.crccnf().write(|w| {
            w.set_len(embassy_nrf::pac::radio::vals::Len::TWO);
            w.set_skipaddr(embassy_nrf::pac::radio::vals::Skipaddr::INCLUDE);
        });
        r.crcpoly().write(|w| w.set_crcpoly(0x1_1021));
        r.crcinit().write(|w| w.set_crcinit(0x0000_FFFF));

        r.modecnf0().write(|w| {
            w.set_ru(embassy_nrf::pac::radio::vals::Ru::FAST);
            w.set_dtx(embassy_nrf::pac::radio::vals::Dtx::B0);
        });

        r.frequency().write(|w| {
            w.set_frequency(80);
        });

        embassy_nrf::interrupt::typelevel::RADIO::unpend();

        unsafe {
            embassy_nrf::interrupt::typelevel::RADIO::enable();
        }

        info!("Radio configured!");
        Self {
            _radio,
            id: [0u8; 2],
        }
    }

    pub fn set_tx_addresses(&mut self, f: impl FnOnce(&mut Txaddress)) {
        let r = embassy_nrf::pac::RADIO;
        r.txaddress().write(f);
    }

    pub fn set_rx_addresses(&mut self, f: impl FnOnce(&mut Rxaddresses)) {
        let r = embassy_nrf::pac::RADIO;
        r.rxaddresses().write(f);
    }
    fn get_next_id(&mut self, idx: usize) -> u8 {
        self.id[idx] = self.id[idx].wrapping_add(1);
        self.id[idx]
    }

    pub async fn run_receive(&mut self) {
        loop {
            let mut packet = Packet::default();
            if let Ok(addr) = self.receive(&mut packet).await {
                let key_states = u32::from_le_bytes(packet[0..4].try_into().unwrap());
                CHANNEL.send((addr, key_states)).await;
            }
        }
    }

    pub async fn receive(&mut self, packet: &mut Packet) -> Result<u8, ()> {
        let r = embassy_nrf::pac::RADIO;
        r.packetptr().write_value(packet.buffer.as_mut_ptr() as u32);
        loop {
            self.receive_inner().await?;
            let addr = r.rxmatch().read().rxmatch();
            let idx = addr as usize - 1;
            if self.id[idx] != packet.id() {
                self.id[idx] = packet.id();
                return Ok(addr);
            }
        }
    }

    async fn receive_inner(&mut self) -> Result<(), ()> {
        let r = embassy_nrf::pac::RADIO;
        r.shorts().write(|w| {
            w.set_ready_start(true);
            w.set_end_disable(true);
        });

        compiler_fence(core::sync::atomic::Ordering::Release);
        r.tasks_rxen().write_value(1);

        r.intenclr().write(|w| w.0 = 0xFFFF_FFFF);
        core::future::poll_fn(|cx| {
            STATE.register(cx.waker());
            if r.events_disabled().read() != 0 {
                r.events_disabled().write_value(0);
                info!("Data received!");
                Poll::Ready(())
            } else {
                r.intenset().write(|w| w.set_disabled(true));
                Poll::Pending
            }
        })
        .await;
        compiler_fence(core::sync::atomic::Ordering::Acquire);
        if r.events_crcok().read() != 0 {
            r.events_crcok().write_value(0);
            Ok(())
        } else {
            Err(())
        }
    }

    pub async fn send(&mut self, packet: &mut Packet) {
        let r = embassy_nrf::pac::RADIO;
        r.packetptr().write_value(packet.buffer.as_ptr() as u32);
        packet.set_id(self.get_next_id(0));
        for _ in 0..3 {
            self.send_inner().await;
        }
    }

    pub async fn send_inner(&mut self) {
        let r = embassy_nrf::pac::RADIO;
        r.shorts().write(|w| {
            w.set_ready_start(true);
            w.set_end_disable(true);
        });

        compiler_fence(core::sync::atomic::Ordering::Release);
        r.tasks_txen().write_value(1);
        r.intenclr().write(|w| w.0 = 0xFFFF_FFFF);
        core::future::poll_fn(|cx| {
            STATE.register(cx.waker());
            if r.events_disabled().read() != 0 {
                info!("Data sent!");
                r.events_disabled().write_value(0);
                Poll::Ready(())
            } else {
                r.intenset().write(|w| w.set_disabled(true));
                Poll::Pending
            }
        })
        .await;

        compiler_fence(core::sync::atomic::Ordering::Acquire);
    }
}

pub struct Packet {
    buffer: [u8; BUFFER_SIZE + META_SIZE],
}

impl Packet {
    const LEN_INDEX: usize = 0;
    const ID_INDEX: usize = 1;

    pub const fn default() -> Self {
        Self {
            buffer: [0u8; BUFFER_SIZE + META_SIZE],
        }
    }

    pub fn len(&self) -> usize {
        self.buffer[Self::LEN_INDEX] as usize
    }

    pub fn set_len(&mut self, len: usize) {
        self.buffer[Self::LEN_INDEX] = len as u8;
    }

    pub fn id(&self) -> u8 {
        self.buffer[Self::ID_INDEX]
    }

    pub fn set_id(&mut self, id: u8) {
        self.buffer[Self::ID_INDEX] = id;
    }

    pub fn copy_from_slice(&mut self, src: &[u8]) {
        assert!(src.len() <= BUFFER_SIZE);
        self.buffer[META_SIZE..][..src.len()].copy_from_slice(src);
        self.set_len(src.len());
    }
}

impl core::ops::Deref for Packet {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.buffer[META_SIZE..][..self.len()]
    }
}

impl core::ops::DerefMut for Packet {
    fn deref_mut(&mut self) -> &mut [u8] {
        let len = self.len();
        &mut self.buffer[META_SIZE..][..len]
    }
}

pub async fn receive_channel() -> (u8, u32) {
    CHANNEL.receive().await
}
