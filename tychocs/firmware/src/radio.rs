use core::{
    cell::{Cell, UnsafeCell},
    ptr::null,
    sync::atomic::{compiler_fence, AtomicBool},
    task::Poll,
};

use defmt::info;
use embassy_nrf::{
    interrupt::{
        self,
        typelevel::{self, Interrupt},
    },
    pac::{
        common::R,
        radio::regs::{Rxaddresses, Txaddress},
    },
    Peri,
};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, waitqueue::AtomicWaker,
};
use embassy_time::{Duration, Instant};

const BUFFER_SIZE: usize = 32;
static TRAD_STATE: AtomicWaker = AtomicWaker::new();

pub const DONGLE_ADDRESS: u32 = 0x0A55_0A55;
pub const DONGLE_PREFIX: u8 = 0x42;
pub const KEYBOARD_ADDRESS: u32 = 0x0727_0727;
pub const LEFT_PREFIX: u8 = 0x21;
pub const RIGHT_PREFIX: u8 = 0x25;

const META_SIZE: usize = 3;

enum RadioState {
    Disabled,
    Tx,
    TxAck,
    Rx,
    RxAck,
}

static mut CURRENT_PACKET: Packet = Packet::default();
static mut TX_ID: u8 = 0;
static mut RX_ID: [u8; 8] = [128; 8];
static ACTIVE: AtomicBool = AtomicBool::new(false);
pub struct RadioInterruptHandler {}
static mut RADIO_STATE: RadioState = RadioState::Disabled;

static CHAN: Channel<CriticalSectionRawMutex, (), 10> = Channel::new();
static P_CHAN: Channel<CriticalSectionRawMutex, Packet, 10> = Channel::new();

impl typelevel::Handler<typelevel::RADIO> for RadioInterruptHandler {
    unsafe fn on_interrupt() {
        static mut ACK_PACKET: Packet = Packet::default();
        let r = embassy_nrf::pac::RADIO;
        let t = embassy_nrf::pac::TIMER0;
        match RADIO_STATE {
            RadioState::Disabled => {}
            RadioState::Tx => {
                if r.events_disabled().read() != 0 {
                    r.events_disabled().write_value(0);
                    r.packetptr().write_value(ACK_PACKET.buffer.as_ptr() as u32);
                    RADIO_STATE = RadioState::TxAck;
                    t.tasks_start().write_value(1);
                    compiler_fence(core::sync::atomic::Ordering::Release);
                    r.tasks_rxen().write_value(1);
                }
            }
            RadioState::TxAck => {
                if r.events_disabled().read() != 0 {
                    r.events_disabled().write_value(0);
                    if r.events_crcok().read() != 0 {
                        r.events_crcok().write_value(0);
                        if ACK_PACKET.packet_type().unwrap() == PacketType::Ack
                            && ACK_PACKET.id() == TX_ID
                        {
                            TX_ID += 1;
                            RADIO_STATE = RadioState::Disabled;
                            t.tasks_stop().write_value(1);
                            let _ = CHAN.try_send(());
                        }
                    } else {
                        RADIO_STATE = RadioState::Tx;
                        r.events_crcok().write_value(0);
                        r.packetptr()
                            .write_value(CURRENT_PACKET.buffer.as_ptr() as u32);
                        compiler_fence(core::sync::atomic::Ordering::Release);
                        t.tasks_stop().write_value(1);
                        r.tasks_txen().write_value(1);
                    }
                }
            }
            RadioState::Rx => {
                if r.events_disabled().read() != 0 {
                    r.events_disabled().write_value(0);
                    if r.events_crcok().read() != 0 {
                        r.events_crcok().write_value(0);
                        if CURRENT_PACKET.packet_type().unwrap() == PacketType::Data {
                            RADIO_STATE = RadioState::RxAck;
                            ACK_PACKET.set_len(1);
                            ACK_PACKET.set_type(PacketType::Ack);
                            ACK_PACKET.set_id(CURRENT_PACKET.id());
                            r.packetptr().write_value(ACK_PACKET.buffer.as_ptr() as u32);
                            compiler_fence(core::sync::atomic::Ordering::Release);
                            r.tasks_txen().write_value(1);
                        } else {
                            r.tasks_rxen().write_value(1);
                        }
                    } else {
                        r.tasks_rxen().write_value(1);
                    }
                }
            }
            RadioState::RxAck => {
                if r.events_disabled().read() != 0 {
                    let addr = r.rxmatch().read().rxmatch() as usize;
                    r.events_disabled().write_value(0);
                    r.packetptr()
                        .write_value(CURRENT_PACKET.buffer.as_ptr() as u32);
                    CURRENT_PACKET.addr = addr as u8;
                    if CURRENT_PACKET.id() != RX_ID[addr] {
                        RADIO_STATE = RadioState::Rx;
                        RX_ID[addr] = CURRENT_PACKET.id();
                        P_CHAN.try_send(CURRENT_PACKET);
                        r.tasks_rxen().write_value(1);
                    } else {
                        RADIO_STATE = RadioState::Rx;
                        r.tasks_rxen().write_value(1);
                    }
                }
            }
        }
    }
}

pub struct RadioTimerInterrupt;

impl typelevel::Handler<typelevel::TIMER0> for RadioTimerInterrupt {
    unsafe fn on_interrupt() {
        let t = embassy_nrf::pac::TIMER0;
        let r = embassy_nrf::pac::RADIO;
        match RADIO_STATE {
            RadioState::Disabled => {
                if t.events_compare(0).read() != 0 {
                    t.events_compare(0).write_value(0);
                }
                t.tasks_stop().write_value(1);
                t.tasks_clear().write_value(1);
            }
            RadioState::Tx => {
                if t.events_compare(0).read() != 0 {
                    t.events_compare(0).write_value(0);
                }
                t.tasks_stop().write_value(1);
                t.tasks_clear().write_value(1);
            }
            RadioState::TxAck => {
                t.tasks_stop().write_value(1);
                t.tasks_clear().write_value(1);
                if t.events_compare(0).read() != 0 {
                    t.events_compare(0).write_value(0);
                    r.tasks_disable().write_value(1);
                    RADIO_STATE = RadioState::Disabled;
                    while r.state().read().state()
                        != embassy_nrf::radio::ieee802154::RadioState::Disabled
                    {
                    }
                    r.events_disabled().write_value(0);

                    RADIO_STATE = RadioState::Tx;
                    r.packetptr()
                        .write_value(CURRENT_PACKET.buffer.as_ptr() as u32);
                    compiler_fence(core::sync::atomic::Ordering::Release);
                    r.tasks_txen().write_value(1);
                }
            }
            RadioState::Rx => todo!(),
            RadioState::RxAck => todo!(),
        }
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
    tx_addreses: u8,
    rx_addresses: u32,
}

impl<'d> Radio<'d> {
    pub fn new(
        _radio: Peri<'d, embassy_nrf::peripherals::RADIO>,
        _timer: Peri<'d, embassy_nrf::peripherals::TIMER0>,
        _irq: impl typelevel::Binding<embassy_nrf::interrupt::typelevel::RADIO, RadioInterruptHandler>,
        _irq_t: impl typelevel::Binding<embassy_nrf::interrupt::typelevel::TIMER0, RadioTimerInterrupt>,
        addresses: Addresses,
    ) -> Self {
        let t = embassy_nrf::pac::TIMER0;
        let r = embassy_nrf::pac::RADIO;

        r.power().write(|w| w.set_power(false));
        r.power().write(|w| w.set_power(true));

        r.mode()
            .write(|w| w.set_mode(embassy_nrf::pac::radio::vals::Mode::Nrf1mbit));

        r.pcnf0().write(|w| {
            w.set_lflen(8);
            w.set_s0len(false);
            w.set_s1len(0);
            w.set_s1incl(embassy_nrf::pac::radio::vals::S1incl::Automatic);
            w.set_plen(embassy_nrf::pac::radio::vals::Plen::_8bit);
        });

        r.pcnf1().write(|w| {
            w.set_maxlen(BUFFER_SIZE as u8);
            w.set_statlen(0);
            w.set_balen(4);
            w.set_endian(embassy_nrf::pac::radio::vals::Endian::Little);
        });

        r.base0().write_value(addresses.base[0]);
        r.base1().write_value(addresses.base[1]);
        r.prefix0()
            .write(|w| w.0 = u32::from_le_bytes(addresses.prefix[0]));
        r.prefix1()
            .write(|w| w.0 = u32::from_le_bytes(addresses.prefix[1]));

        r.crccnf().write(|w| {
            w.set_len(embassy_nrf::pac::radio::vals::Len::Two);
            w.set_skipaddr(embassy_nrf::pac::radio::vals::Skipaddr::Include);
        });
        r.crcpoly().write(|w| w.set_crcpoly(0x1_1021));
        r.crcinit().write(|w| w.set_crcinit(0x0000_FFFF));

        r.modecnf0().write(|w| {
            w.set_ru(embassy_nrf::pac::radio::vals::Ru::Fast);
            w.set_dtx(embassy_nrf::pac::radio::vals::Dtx::B0);
        });

        r.frequency().write(|w| {
            w.set_frequency(80);
        });

        r.shorts().write(|w| {
            w.set_ready_start(true);
            w.set_end_disable(true);
        });

        embassy_nrf::interrupt::typelevel::RADIO::unpend();

        unsafe {
            embassy_nrf::interrupt::typelevel::RADIO::enable();
        }
        r.intenset().write(|w| w.set_disabled(true));

        t.mode()
            .write(|w| w.set_mode(embassy_nrf::pac::timer::vals::Mode::Timer));
        t.bitmode()
            .write(|w| w.set_bitmode(embassy_nrf::pac::timer::vals::Bitmode::_32bit));
        t.prescaler().write(|w| w.set_prescaler(4));
        t.cc(0).write_value(160);

        embassy_nrf::interrupt::typelevel::TIMER0::unpend();
        unsafe {
            embassy_nrf::interrupt::typelevel::TIMER0::enable();
        }

        t.intenset().write(|w| w.set_compare(0, true));
        Self {
            _radio,
            rx_addresses: 0,
            tx_addreses: 0,
        }
    }

    pub fn set_tx_addresses(&mut self, f: impl FnOnce(&mut Txaddress)) {
        let r = embassy_nrf::pac::RADIO;
        r.txaddress().write(f);
        self.tx_addreses = r.txaddress().read().txaddress();
    }

    pub fn set_rx_addresses(&mut self, f: impl FnOnce(&mut Rxaddresses)) {
        let r = embassy_nrf::pac::RADIO;
        r.rxaddresses().write(f);
        self.rx_addresses = r.rxaddresses().read().0;
    }

    pub async fn receive_packet(&mut self) -> Packet {
        let r = embassy_nrf::pac::RADIO;
        cortex_m::interrupt::free(|_cs| unsafe {
            RADIO_STATE = RadioState::Rx;
            r.packetptr()
                .write_value(CURRENT_PACKET.buffer.as_ptr() as u32);
            compiler_fence(core::sync::atomic::Ordering::Release);
            r.tasks_rxen().write_value(1);
        });
        P_CHAN.receive().await
    }

    pub async fn send_packet(&mut self, packet: &Packet) {
        let r = embassy_nrf::pac::RADIO;
        cortex_m::interrupt::free(|_cs| unsafe {
            CURRENT_PACKET = *packet;
            CURRENT_PACKET.set_id(TX_ID);
            CURRENT_PACKET.set_type(PacketType::Data);
            compiler_fence(core::sync::atomic::Ordering::Release);
            r.packetptr()
                .write_value(CURRENT_PACKET.buffer.as_ptr() as u32);
            RADIO_STATE = RadioState::Tx;
            compiler_fence(core::sync::atomic::Ordering::Release);
            r.tasks_txen().write_value(1);
        });
        CHAN.receive().await
    }
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PacketType {
    Data,
    Ack,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Packet {
    pub addr: u8,
    pub buffer: [u8; BUFFER_SIZE + META_SIZE],
}

impl Packet {
    const LEN_INDEX: usize = 0;
    const ID_INDEX: usize = 1;
    const TYPE_INDEX: usize = 2;

    pub const fn default() -> Self {
        Self {
            addr: 0,
            buffer: [(META_SIZE - 1) as u8; BUFFER_SIZE + META_SIZE],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        // Subtract META_SIZE by 1 for len as len field doesn't count the len byte
        self.buffer[Self::LEN_INDEX] as usize - (META_SIZE - 1)
    }

    pub fn set_len(&mut self, len: usize) {
        self.buffer[Self::LEN_INDEX] = (META_SIZE - 1) as u8 + len as u8;
    }

    pub fn id(&self) -> u8 {
        self.buffer[Self::ID_INDEX]
    }

    pub fn set_id(&mut self, id: u8) {
        self.buffer[Self::ID_INDEX] = id;
    }

    pub fn packet_type(&self) -> Result<PacketType, ()> {
        match self.buffer[Self::TYPE_INDEX] {
            0 => Ok(PacketType::Data),
            1 => Ok(PacketType::Ack),
            _ => Err(()),
        }
    }

    pub fn set_type(&mut self, packet_type: PacketType) {
        self.buffer[Self::TYPE_INDEX] = packet_type as u8;
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
