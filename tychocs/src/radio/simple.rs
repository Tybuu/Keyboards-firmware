use defmt::info;
use embassy_futures::select::select;
use embassy_nrf::{
    interrupt,
    pac::radio::regs::{Rxaddresses, Txaddress},
    Peri,
};
use embassy_time::{Duration, Instant, Timer};

use crate::radio::{
    inner_radio::Radio,
    packet::{Packet, PacketType},
    Addresses, InterruptHandler, RECV_CHANNEL, SEND_CHANNEL,
};

const RECEIVE_TIMEOUT: Duration = Duration::from_micros(600);
const TASK_TIMEOUT: Duration = Duration::from_micros(1000);
const ACK_TIMEOUT: Duration = Duration::from_micros(200);
const ADVERTISEMENT_TIMEOUT: Duration = Duration::from_millis(1000);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ConnectionState {
    Scanning,
    Connected,
}

pub struct CRadio<'d> {
    rad: Radio<'d>,
    state: ConnectionState,
    last_time: Instant,
    missed: u32,
}

impl<'d> CRadio<'d> {
    pub fn new(
        radio: Peri<'d, embassy_nrf::peripherals::RADIO>,
        irq: impl interrupt::typelevel::Binding<
            embassy_nrf::interrupt::typelevel::RADIO,
            InterruptHandler,
        >,
        addresses: Addresses,
    ) -> Self {
        Self {
            rad: Radio::new(radio, irq, addresses),
            state: ConnectionState::Scanning,
            last_time: Instant::now(),
            missed: 0,
        }
    }

    pub fn set_tx_addresses(&mut self, f: impl FnOnce(&mut Txaddress)) {
        self.rad.set_tx_addresses(f);
    }

    pub fn set_rx_addresses(&mut self, f: impl FnOnce(&mut Rxaddresses)) {
        self.rad.set_rx_addresses(f);
    }

    pub async fn run(mut self) -> ! {
        loop {
            let state = self.state;
            match state {
                ConnectionState::Scanning => {
                    let packet = self.rad.receive().await;
                    if packet.packet_type().unwrap() == PacketType::Advertise {
                        self.state = ConnectionState::Connected;
                        self.last_time = Instant::now();
                        self.missed = 0;
                        let mut packet = Packet::default();
                        Timer::after_micros(40).await;
                        packet.set_type(PacketType::EstablishConnection);
                        self.rad.send(&packet).await;
                        log::info!("Switching to connected state!");
                    }
                }
                ConnectionState::Connected => {
                    self.last_time += Duration::from_millis(1000);
                    let recv_task = async {
                        let cond = |p: &Packet| p.packet_type().unwrap() == PacketType::Data;
                        if self
                            .rad
                            .receive_with_conditions(RECEIVE_TIMEOUT, cond)
                            .await
                            .is_some()
                        {
                            let mut packet = Packet::default();
                            packet.set_type(PacketType::Ack);
                            Timer::after_micros(40).await;
                            self.rad.send(&packet).await;
                            log::info!("Received pulse!");
                            core::future::pending::<()>().await;
                        } else {
                            log::info!("Missed pulse!");
                            self.missed += 1;
                            if self.missed >= 10 {
                                self.state = ConnectionState::Scanning;
                                log::info!("Switching to scanning state!");
                            } else {
                                core::future::pending::<()>().await;
                            }
                        }
                    };
                    select(Timer::at(self.last_time), recv_task).await;
                }
            }
        }
    }
}

pub struct PRadio<'d> {
    rad: Radio<'d>,
    state: ConnectionState,
    last_time: Instant,
    missed: u32,
}

impl<'d> PRadio<'d> {
    pub fn new(
        radio: Peri<'d, embassy_nrf::peripherals::RADIO>,
        irq: impl interrupt::typelevel::Binding<
            embassy_nrf::interrupt::typelevel::RADIO,
            InterruptHandler,
        >,
        addresses: Addresses,
    ) -> Self {
        Self {
            rad: Radio::new(radio, irq, addresses),
            state: ConnectionState::Scanning,
            last_time: Instant::now(),
            missed: 0,
        }
    }

    pub fn set_tx_addresses(&mut self, f: impl FnOnce(&mut Txaddress)) {
        self.rad.set_tx_addresses(f);
    }

    pub fn set_rx_addresses(&mut self, f: impl FnOnce(&mut Rxaddresses)) {
        self.rad.set_rx_addresses(f);
    }

    pub async fn run(mut self) -> ! {
        loop {
            let state = self.state;
            match state {
                ConnectionState::Scanning => {
                    let adv_task = async {
                        let mut adv_packet = Packet::default();
                        adv_packet.set_type(PacketType::Advertise);
                        let cond = |packet: &Packet| {
                            packet.packet_type().unwrap() == PacketType::EstablishConnection
                        };
                        self.rad.send(&adv_packet).await;
                        if self
                            .rad
                            .receive_with_conditions(RECEIVE_TIMEOUT, cond)
                            .await
                            .is_some()
                        {
                            log::info!("Established connection!");
                            self.state = ConnectionState::Connected;
                            self.last_time = Instant::now();
                            self.missed = 0;
                        } else {
                            log::info!("Unable to establish connection!");
                            // await for timeout as no connection was established
                            core::future::pending::<()>().await;
                        }
                    };
                    select(Timer::after(ADVERTISEMENT_TIMEOUT), adv_task).await;
                }
                ConnectionState::Connected => {
                    self.last_time += Duration::from_millis(1000);
                    let send_task = async {
                        let mut dummy_packet = Packet::default();
                        dummy_packet.set_type(PacketType::Data);
                        self.rad.send(&dummy_packet).await;
                        let cond = |p: &Packet| p.packet_type().unwrap() == PacketType::Ack;
                        if self
                            .rad
                            .receive_with_conditions(RECEIVE_TIMEOUT, cond)
                            .await
                            .is_some()
                        {
                            log::info!("Received pulse!");
                            core::future::pending::<()>().await;
                        } else {
                            log::info!("Missed pulse!");
                            self.missed += 1;
                            if self.missed >= 10 {
                                self.state = ConnectionState::Scanning;
                                log::info!("Switching to scanning state!");
                            } else {
                                core::future::pending::<()>().await;
                            }
                        }
                    };
                    select(Timer::at(self.last_time), send_task).await;
                }
            }
        }
    }
}
