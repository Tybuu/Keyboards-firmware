use core::{future::Future, sync::atomic::compiler_fence, task::Poll};

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
use embassy_time::{Duration, Timer};

use crate::radio::{
    packet::{Packet, BUFFER_SIZE},
    Addresses, InterruptHandler, STATE,
};

pub(crate) struct Radio<'d> {
    _radio: Peri<'d, embassy_nrf::peripherals::RADIO>,
}

impl<'d> Radio<'d> {
    pub(crate) fn new(
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
            w.set_s1len(0);
            w.set_s1incl(embassy_nrf::pac::radio::vals::S1incl::AUTOMATIC);
            w.set_plen(embassy_nrf::pac::radio::vals::Plen::_8BIT);
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
        Self { _radio }
    }

    pub(crate) fn txaddress(&self) -> u8 {
        let r = embassy_nrf::pac::RADIO;
        r.txaddress().read().txaddress()
    }

    pub(crate) fn rx_addresses(&self) -> u8 {
        let r = embassy_nrf::pac::RADIO;
        r.rxaddresses().read().0 as u8
    }

    pub(crate) async fn receive(&mut self) -> Packet {
        let mut packet = Packet::default();
        loop {
            if ReceiveFuture::new(&mut packet).await.is_ok() {
                break;
            }
        }
        packet
    }

    pub(crate) async fn receive_with_conditions(
        &mut self,
        timeout: Duration,
        f: impl Fn(&Packet) -> bool,
    ) -> Option<Packet> {
        let receive_task = async {
            loop {
                let mut packet = Packet::default();
                let res = ReceiveFuture::new(&mut packet).await;
                if res.is_ok() && f(&packet) {
                    return packet;
                }
            }
        };
        match select(Timer::after(timeout), receive_task).await {
            embassy_futures::select::Either::First(_) => None,
            embassy_futures::select::Either::Second(packet) => Some(packet),
        }
    }

    pub(crate) async fn send(&mut self, packet: &Packet) {
        SendFuture::new(packet).await;
    }

    pub(crate) fn set_tx_addresses(&mut self, f: impl FnOnce(&mut Txaddress)) {
        let r = embassy_nrf::pac::RADIO;
        r.txaddress().write(f);
    }

    pub(crate) fn set_rx_addresses(&mut self, f: impl FnOnce(&mut Rxaddresses)) {
        let r = embassy_nrf::pac::RADIO;
        r.rxaddresses().write(f);
    }
}

struct SendFuture<'a> {
    complete: bool,
    init: bool,
    packet: &'a Packet,
}

impl<'a> SendFuture<'a> {
    fn new(packet: &'a Packet) -> SendFuture<'a> {
        Self {
            complete: false,
            init: false,
            packet,
        }
    }

    fn init(&mut self) {
        if !self.init {
            self.init = true;
            let r = embassy_nrf::pac::RADIO;
            r.shorts().write(|w| {
                w.set_ready_start(true);
                w.set_end_disable(true);
            });
            r.packetptr()
                .write_value(self.packet.buffer.as_ptr() as u32);

            compiler_fence(core::sync::atomic::Ordering::Release);
            r.tasks_txen().write_value(1);
            r.intenclr().write(|w| w.0 = 0xFFFF_FFFF);
        }
    }
}

impl<'a> Future for SendFuture<'a> {
    type Output = ();

    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<Self::Output> {
        let r = embassy_nrf::pac::RADIO;
        self.init();
        STATE.register(cx.waker());
        if r.events_disabled().read() != 0 {
            info!("Data sent!");
            r.events_disabled().write_value(0);
            self.complete = true;
            Poll::Ready(())
        } else {
            r.intenset().write(|w| w.set_disabled(true));
            Poll::Pending
        }
    }
}

impl<'a> Drop for SendFuture<'a> {
    fn drop(&mut self) {
        if !self.complete {
            let r = embassy_nrf::pac::RADIO;
            r.tasks_disable().write_value(1);
            while r.state().read().state() != RadioState::DISABLED {}
            r.events_disabled().write_value(0);
        }
    }
}

struct ReceiveFuture<'a> {
    complete: bool,
    init: bool,
    packet: &'a mut Packet,
}

impl<'a> ReceiveFuture<'a> {
    fn new(packet: &'a mut Packet) -> ReceiveFuture<'a> {
        Self {
            complete: false,
            init: false,
            packet,
        }
    }

    fn init(&mut self) {
        if !self.init {
            self.init = true;
            let r = embassy_nrf::pac::RADIO;
            r.shorts().write(|w| {
                w.set_ready_start(true);
                w.set_end_disable(true);
            });
            r.packetptr()
                .write_value(self.packet.buffer.as_mut_ptr() as u32);

            compiler_fence(core::sync::atomic::Ordering::Release);
            r.tasks_rxen().write_value(1);
            r.intenclr().write(|w| w.0 = 0xFFFF_FFFF);
        }
    }
}

impl<'a> Future for ReceiveFuture<'a> {
    type Output = Result<(), ()>;
    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<Self::Output> {
        self.init();
        let r = embassy_nrf::pac::RADIO;
        STATE.register(cx.waker());
        if r.events_disabled().read() != 0 {
            r.events_disabled().write_value(0);
            self.complete = true;
            if r.events_crcok().read() != 0 {
                r.events_crcok().write_value(0);
                self.packet.addr = r.rxmatch().read().rxmatch();
                Poll::Ready(Ok(()))
            } else {
                Poll::Ready(Err(()))
            }
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
