use core::{
    future::Future,
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
use num_enum::{TryFromPrimitive, TryFromPrimitiveError};

use crate::{DONGLE_ADDRESS, DONGLE_PREFIX, KEYBOARD_ADDRESS, LEFT_PREFIX, RIGHT_PREFIX};

const BUFFER_SIZE: usize = 32;
const META_SIZE: usize = 3;

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
    tx_addreses: u8,
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
            .write(|w| w.set_mode(embassy_nrf::pac::radio::vals::Mode::NRF_1MBIT));

        r.pcnf0().write(|w| {
            w.set_lflen(8);
            w.set_s0len(false);
            w.set_s1len(0);
            w.set_s1incl(embassy_nrf::pac::radio::vals::S1incl::AUTOMATIC);
            w.set_plen(embassy_nrf::pac::radio::vals::Plen::_8BIT);
        });

        r.pcnf1().write(|w| {
            w.set_maxlen(BUFFER_SIZE as u8);
            w.set_statlen(0);
            w.set_balen(4);
            w.set_whiteen(true);
            w.set_endian(embassy_nrf::pac::radio::vals::Endian::LITTLE);
        });

        r.datawhiteiv().write(|w| w.set_datawhiteiv(80));

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

    async fn await_clear(&mut self) {
        let r = embassy_nrf::pac::RADIO;
        r.shorts().write(|w| {});
        r.rxaddresses().write(|w| w.0 = 0);

        r.tasks_rxen().write_value(1);
        while r.events_ready().read() == 0 {}
        r.events_ready().write_value(0);
        r.tasks_start().write_value(1);
        r.tasks_rssistart().write_value(1);
        let mut rssi_val = 0;
        while rssi_val < 75 {
            Timer::after_micros(10).await;
            rssi_val = r.rssisample().read().rssisample();
        }

        r.tasks_rssistop().write_value(1);
        while r.events_rssiend().read() == 0 {}
        r.events_rssiend().write_value(0);
        r.tasks_disable().write_value(1);
        while r.events_disabled().read() == 0 {}
        r.events_disabled().write_value(0);
    }

    async fn transmit_ack(&mut self, id: u8) {
        let mut packet = Packet::default();
        packet.set_type(PacketType::Ack);
        packet.set_len(1);
        packet.set_id(id);
        self.send_inner(&mut packet).await;
    }

    async fn await_ack(&mut self, id: u8) -> Result<(), ()> {
        let r = embassy_nrf::pac::RADIO;
        let mut packet = Packet::default();
        let addr = self.tx_addreses;
        r.packetptr().write_value(packet.buffer.as_mut_ptr() as u32);
        let receive_task = async {
            loop {
                if ReceiveFuture::new(&mut packet).await.is_ok()
                    && packet.packet_type().unwrap() == PacketType::Ack
                    && packet.id() == id
                    && packet[0] == addr
                {
                    break;
                };
            }
        };
        match select(Timer::after_micros(300), receive_task).await {
            embassy_futures::select::Either::First(_) => Err(()),
            embassy_futures::select::Either::Second(_) => Ok(()),
        }
    }

    async fn send(&mut self, packet: &mut Packet) {
        self.tx_id = self.tx_id.wrapping_add(1);
        packet.set_id(self.tx_id);
        packet.set_type(PacketType::Data);
        for _ in 0..10 {
            // self.await_clear().await;
            self.send_inner(packet).await;
            if self.await_ack(packet.id()).await.is_ok() {
                return;
            }
        }
    }

    async fn receive(&mut self, packet: &mut Packet) {
        let r = embassy_nrf::pac::RADIO;
        loop {
            let res = ReceiveFuture::new(packet).await;
            if res.is_ok() && packet.packet_type().unwrap() == PacketType::Data {
                let addr = r.rxmatch().read().rxmatch();
                self.transmit_ack(packet.id()).await;

                // If packet_id is the same as the previous id, it must mean that the ack hasn't
                // gone through so we'll discard the packet on the receiving end but send another
                // ack to make sure the tx side knows the packet was already received
                if packet.id() != self.rx_id[addr as usize] {
                    self.rx_id[addr as usize] = packet.id();
                    packet.addr = addr;
                    return;
                }
            }
        }
    }

    async fn send_inner(&mut self, packet: &mut Packet) {
        let r = embassy_nrf::pac::RADIO;

        r.packetptr().write_value(packet.buffer.as_ptr() as u32);
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
        self.tx_addreses = r.txaddress().read().txaddress();
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
                    let addr = pipe.packet.addr;
                    self.send(&mut pipe.packet).await;
                }
                Direction::Rx => {
                    self.receive(&mut pipe.packet).await;
                    FROM_SINGLETON.signal(pipe);
                }
            }
        }
    }
}

struct ReceiveFuture<'a> {
    complete: bool,
    packet: &'a mut Packet,
}

impl<'a> ReceiveFuture<'a> {
    fn new(packet: &'a mut Packet) -> ReceiveFuture<'a> {
        let r = embassy_nrf::pac::RADIO;
        r.shorts().write(|w| {
            w.set_ready_start(true);
            w.set_end_disable(true);
        });
        r.packetptr().write_value(packet.buffer.as_ptr() as u32);

        compiler_fence(core::sync::atomic::Ordering::Release);
        r.tasks_rxen().write_value(1);
        r.intenclr().write(|w| w.0 = 0xFFFF_FFFF);

        Self {
            complete: false,
            packet,
        }
    }
}

impl<'a> Future for ReceiveFuture<'a> {
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
            self.packet.addr = r.rxmatch().read().rxmatch();
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

impl<'a> Drop for ReceiveFuture<'a> {
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
}

pub struct RadioClient {}

impl RadioClient {
    pub async fn mutate_packet(&self) -> MutexGuard<'static, CriticalSectionRawMutex, Packet> {
        let mut packet = DATA.lock().await;
        *packet = Packet::default();
        packet
    }

    pub async fn send_packet(&self, packet: MutexGuard<'static, CriticalSectionRawMutex, Packet>) {
        let pipe = Pipe {
            packet,
            direction: Direction::Tx,
        };
        TO_SINGLETON.send(pipe).await;
    }
    pub async fn receive_packet(&self) -> MutexGuard<'static, CriticalSectionRawMutex, Packet> {
        let packet = DATA.lock().await;
        let pipe = Pipe {
            packet,
            direction: Direction::Rx,
        };
        TO_SINGLETON.send(pipe).await;
        let res = FROM_SINGLETON.wait().await;
        res.packet
    }
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, TryFromPrimitive)]
enum PacketType {
    Data,
    Ack,
}

pub struct Packet {
    pub addr: u8,
    buffer: [u8; BUFFER_SIZE + META_SIZE],
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

    fn packet_type(&self) -> Result<PacketType, TryFromPrimitiveError<PacketType>> {
        self.buffer[Self::TYPE_INDEX].try_into()
    }

    fn set_type(&mut self, packet_type: PacketType) {
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
