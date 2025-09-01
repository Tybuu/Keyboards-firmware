use core::{
    cell::RefCell,
    future::Future,
    ops::DerefMut,
    sync::atomic::{compiler_fence, AtomicBool},
    task::Poll,
};

use defmt::info;
use embassy_futures::select::select;
use embassy_nrf::{
    interrupt::{
        self,
        typelevel::{self, Interrupt},
    },
    pac::radio::regs::{Rxaddresses, Txaddress},
    radio::ieee802154::RadioState,
    Peri,
};
use embassy_sync::{
    blocking_mutex::raw::{CriticalSectionRawMutex, ThreadModeRawMutex},
    channel::Channel,
    mutex::{Mutex, MutexGuard},
    signal::Signal,
    waitqueue::AtomicWaker,
};
use embassy_time::Timer;

use crate::{DONGLE_ADDRESS, DONGLE_PREFIX, KEYBOARD_ADDRESS, LEFT_PREFIX, RIGHT_PREFIX};

const BUFFER_SIZE: usize = 32;
const META_SIZE: usize = 2;

static STATE: AtomicWaker = AtomicWaker::new();

static DATA: Mutex<CriticalSectionRawMutex, Packet> = Mutex::new(Packet::default());
static TO_SINGLETON: Channel<CriticalSectionRawMutex, Pipe<'static>, 1> = Channel::new();
static FROM_SINGLETON: Signal<CriticalSectionRawMutex, Pipe<'static>> = Signal::new();

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
    tx_addreses: u32,
    rx_addresses: u32,
    rx_id: [u8; 8],
    tx_id: u8,
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
            w.set_s1len(8);
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
            rx_addresses: 0,
            tx_addreses: 0,
            rx_id: [0u8; 8],
            tx_id: 0u8,
        }
    }

    async fn transmit_ack(&mut self) {
        let r = embassy_nrf::pac::RADIO;
        let mut packet = Packet::default();
        packet.set_len(1);
        r.packetptr().write_value(packet.as_ptr() as u32);
        self.send_inner().await;
    }

    async fn await_ack(&mut self, addr: u8) -> Result<(), ()> {
        let r = embassy_nrf::pac::RADIO;
        let mut packet = Packet::default();
        packet.set_len(1);
        r.rxaddresses().write(|w| w.0 = 1 << addr);
        r.packetptr().write_value(packet.as_mut_ptr() as u32);
        match select(Timer::after_micros(150), self.receive_inner()).await {
            embassy_futures::select::Either::First(_) => Err(()),
            embassy_futures::select::Either::Second(res) => res,
        }
    }

    async fn send(&mut self, packet: &mut Packet, addr: u8) {
        let r = embassy_nrf::pac::RADIO;
        self.tx_id = self.tx_id.wrapping_add(1);
        packet.set_id(self.tx_id);
        loop {
            r.packetptr().write_value(packet.buffer.as_ptr() as u32);
            self.send_inner().await;
            if self.await_ack(addr).await.is_ok() {
                r.rxaddresses().write(|w| w.0 = self.rx_addresses);
                return;
            }
        }
    }

    async fn receive(&mut self, packet: &mut Packet) -> u8 {
        let r = embassy_nrf::pac::RADIO;
        loop {
            r.packetptr().write_value(packet.buffer.as_mut_ptr() as u32);
            let res = self.receive_inner().await;
            if res.is_ok() {
                let addr = r.rxmatch().read().rxmatch();
                self.transmit_ack().await;

                // If packet_id is the same as the previous id, it must mean that the ack hasn't
                // gone through so we'll discard the packet on the receiving end but send another
                // ack to make sure the tx side knows the packet was already received
                if packet.id() != self.rx_id[addr as usize] {
                    self.rx_id[addr as usize] = packet.id();
                    return addr;
                }
            }
        }
    }

    fn receive_inner(&mut self) -> ReceiveFuture {
        let r = embassy_nrf::pac::RADIO;
        r.shorts().write(|w| {
            w.set_ready_start(true);
            w.set_end_disable(true);
        });

        compiler_fence(core::sync::atomic::Ordering::Release);
        r.tasks_rxen().write_value(1);

        r.intenclr().write(|w| w.0 = 0xFFFF_FFFF);
        ReceiveFuture::new()
    }

    async fn send_inner(&mut self) {
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

    pub fn set_tx_addresses(&mut self, f: impl FnOnce(&mut Txaddress)) {
        let r = embassy_nrf::pac::RADIO;
        r.txaddress().write(f);
        self.tx_addreses = r.txaddress().read().0;
    }

    pub fn set_rx_addresses(&mut self, f: impl FnOnce(&mut Rxaddresses)) {
        let r = embassy_nrf::pac::RADIO;
        r.rxaddresses().write(f);
        self.rx_addresses = r.rxaddresses().read().0;
    }

    pub async fn run(mut self) {
        loop {
            let mut pipe = TO_SINGLETON.receive().await;
            match pipe.direction {
                Direction::Tx => {
                    self.send(&mut pipe.packet, pipe.address).await;
                }
                Direction::Rx => {
                    pipe.address = self.receive(&mut pipe.packet).await;
                    FROM_SINGLETON.signal(pipe);
                }
            }
        }
    }
}

struct ReceiveFuture {
    complete: bool,
}

impl ReceiveFuture {
    fn new() -> ReceiveFuture {
        Self { complete: false }
    }
}

impl Future for ReceiveFuture {
    type Output = Result<(), ()>;
    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<Self::Output> {
        let r = embassy_nrf::pac::RADIO;
        STATE.register(cx.waker());
        if r.events_disabled().read() != 0 {
            info!("Data sent!");
            r.events_disabled().write_value(0);
            let res = if r.events_crcok().read() != 0 {
                r.events_crcok().write_value(0);
                Ok(())
            } else {
                Err(())
            };
            self.complete = true;
            Poll::Ready(res)
        } else {
            r.intenset().write(|w| w.set_disabled(true));
            Poll::Pending
        }
    }
}

impl Drop for ReceiveFuture {
    fn drop(&mut self) {
        if !self.complete {
            let r = embassy_nrf::pac::RADIO;
            r.tasks_disable().write_value(1);
            while r.state().read().state() != RadioState::DISABLED {}
            r.events_disabled().write_value(0);
        }
    }
}

enum Direction {
    Tx,
    Rx,
}

pub struct Pipe<'a> {
    packet: MutexGuard<'a, CriticalSectionRawMutex, Packet>,
    direction: Direction,
    address: u8,
}

pub struct RadioClient {}

impl RadioClient {
    pub async fn mutate_packet(&self) -> MutexGuard<'static, CriticalSectionRawMutex, Packet> {
        let mut packet = DATA.lock().await;
        *packet = Packet::default();
        packet
    }

    pub async fn send_packet(
        &self,
        packet: MutexGuard<'static, CriticalSectionRawMutex, Packet>,
        address: u8,
    ) {
        let pipe = Pipe {
            packet,
            direction: Direction::Tx,
            address,
        };
        TO_SINGLETON.send(pipe).await;
    }
    pub async fn receive_packet(
        &self,
    ) -> (MutexGuard<'static, CriticalSectionRawMutex, Packet>, u8) {
        let packet = DATA.lock().await;
        let pipe = Pipe {
            packet,
            direction: Direction::Rx,
            address: 0,
        };
        TO_SINGLETON.send(pipe).await;
        let res = FROM_SINGLETON.wait().await;
        (res.packet, res.address)
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
