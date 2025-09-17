use embassy_futures::{join::join, select::select};
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
const ADVERTISEMENT_TIMEOUT: Duration = Duration::from_millis(500);

const NUM_CONNECTIONS: usize = 1;
const NUM_RETRIES: usize = 3;

const MAX_CONNECTION_EVENTS: u32 = 500;
const MAX_MISSED_EVENTS: u32 = 5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ConnectionState {
    Advertisement,
    ConnectedReceive,
    ConnectedSend,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct CentralConnection {
    state: ConnectionState,
    num_events: u32,
    num_miss_events: u32,
    addr: u8,
    rx_id: u8,
}

impl CentralConnection {
    const fn default() -> Self {
        Self {
            state: ConnectionState::Advertisement,
            num_events: 0,
            num_miss_events: 0,
            addr: 0,
            rx_id: 0,
        }
    }

    async fn handle_connection<'d>(&mut self, rad: &mut Radio<'d>) {
        let state = self.state;
        match state {
            ConnectionState::Advertisement => {
                let cond = |packet: &Packet| packet.packet_type().unwrap() == PacketType::Advertise;

                let mut establish_packet = Packet::default();
                establish_packet.set_type(PacketType::EstablishConnection);

                if let Some(packet) = rad.receive_with_conditions(RECEIVE_TIMEOUT, cond).await {
                    establish_packet.set_id(packet.addr);
                    rad.send(&establish_packet).await;

                    self.state = ConnectionState::ConnectedReceive;
                    self.addr = packet.addr;
                    self.num_events = 0;
                    self.num_miss_events = 0;
                    //log::info!("Established connection with addr {}", self.addr);
                }
            }
            ConnectionState::ConnectedReceive => {
                let cond = |packet: &Packet| {
                    packet.packet_type().unwrap() == PacketType::Data
                        && packet.addr == self.addr
                        && packet.id() != self.rx_id
                };

                if let Some(packet) = rad.receive_with_conditions(RECEIVE_TIMEOUT, cond).await {
                    //log::info!("Packet received from addr {}", self.addr);
                    let mut ack_packet = Packet::default();
                    ack_packet.set_id(packet.id());
                    ack_packet.set_type(PacketType::Ack);
                    ack_packet.set_len(1);
                    ack_packet[0] = self.addr;
                    rad.send(&ack_packet).await;

                    self.rx_id = packet.id();

                    // Push out the earliest packet to make space for the newest packet if channel
                    // is full
                    if RECV_CHANNEL.try_send(packet).is_err() {
                        RECV_CHANNEL.try_receive();
                        RECV_CHANNEL.try_send(packet);
                    }
                }

                self.num_events += 1;
                if self.num_events >= MAX_CONNECTION_EVENTS {
                    self.state = ConnectionState::ConnectedSend;
                    self.num_events = 0;
                }
            }
            ConnectionState::ConnectedSend => {
                let mut packet = if let Ok(packet) = SEND_CHANNEL.try_receive() {
                    packet
                } else {
                    Packet::default()
                };

                packet.set_type(PacketType::Data);
                packet.set_id(self.addr);

                let mut ack_received = false;
                for _ in 0..NUM_RETRIES {
                    rad.send(&packet).await;

                    let cond = |packet: &Packet| {
                        packet.packet_type().unwrap() == PacketType::Ack && packet.addr == self.addr
                    };

                    if rad
                        .receive_with_conditions(ACK_TIMEOUT, cond)
                        .await
                        .is_some()
                    {
                        ack_received = true;
                        break;
                    }
                }

                if ack_received {
                    //log::info!("Ack received");
                    self.num_miss_events = 0
                } else {
                    //log::info!("No ack received");
                    self.num_miss_events += 1;
                }

                if self.num_miss_events >= MAX_MISSED_EVENTS {
                    //log::info!("switching to scanning");
                    self.state = ConnectionState::Advertisement;
                } else {
                    self.state = ConnectionState::ConnectedReceive;
                }
            }
        }
    }
}

pub struct RadioCentral<'d> {
    pub rad: Radio<'d>,
    connections: [CentralConnection; NUM_CONNECTIONS],
}

impl<'d> RadioCentral<'d> {
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
            connections: [CentralConnection::default(); NUM_CONNECTIONS],
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
            for connection in &mut self.connections {
                join(
                    connection.handle_connection(&mut self.rad),
                    Timer::after(TASK_TIMEOUT),
                )
                .await;
            }
        }
    }
}

pub struct RadioPerp<'d> {
    rad: Radio<'d>,
    state: ConnectionState,
    tx_id: u8,
    num_missed_events: u32,
    prev_recv_time: Instant,
}

impl<'d> RadioPerp<'d> {
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
            state: ConnectionState::Advertisement,
            tx_id: 0,
            num_missed_events: 0,
            prev_recv_time: Instant::now(),
        }
    }

    pub fn set_tx_addresses(&mut self, f: impl FnOnce(&mut Txaddress)) {
        self.rad.set_tx_addresses(f);
    }

    pub fn set_rx_addresses(&mut self, f: impl FnOnce(&mut Rxaddresses)) {
        self.rad.set_rx_addresses(f);
    }

    fn get_next_tx_id(&mut self) -> u8 {
        self.tx_id = self.tx_id.wrapping_add(1);
        self.tx_id
    }

    pub async fn run(mut self) -> ! {
        loop {
            let state = self.state;
            match state {
                ConnectionState::Advertisement => {
                    let task = async {
                        let mut adv_packet = Packet::default();
                        adv_packet.set_type(PacketType::Advertise);
                        let tx_addr = self.rad.txaddress();
                        let rx_addr = self.rad.rx_addresses();
                        //log::info!("TxAddr: {}, RxAddr: {:08b}", tx_addr, rx_addr);
                        let cond = |packet: &Packet| {
                            packet.packet_type().unwrap() == PacketType::EstablishConnection
                                && packet.id() == tx_addr
                        };
                        for _ in 0..NUM_RETRIES {
                            self.rad.send(&adv_packet).await;

                            if self
                                .rad
                                .receive_with_conditions(ACK_TIMEOUT, cond)
                                .await
                                .is_some()
                            {
                                //log::info!("Established connection!");
                                self.state = ConnectionState::ConnectedSend;
                                self.prev_recv_time = Instant::now();
                                self.tx_id = 0;
                                self.num_missed_events = 0;
                                // Skip the timeout as we've already established a connection
                                return;
                            }
                        }
                        //log::info!("Unable to establish connection!");
                        // await for timeout as no connection was established
                        core::future::pending::<()>().await;
                    };

                    select(Timer::after(ADVERTISEMENT_TIMEOUT), task).await;
                }
                ConnectionState::ConnectedReceive => {
                    let addr = self.rad.txaddress();
                    let cond = |packet: &Packet| {
                        packet.packet_type().unwrap() == PacketType::Data && packet.id() != addr
                    };

                    if let Some(packet) = self
                        .rad
                        .receive_with_conditions(RECEIVE_TIMEOUT, cond)
                        .await
                    {
                        //log::info!("Data received from central!");
                        self.prev_recv_time = Instant::now();
                        let mut ack_packet = Packet::default();
                        ack_packet.set_id(packet.id());
                        ack_packet.set_type(PacketType::Ack);
                        self.rad.send(&ack_packet).await;

                        if packet.len() != 0 {
                            // Push out the earliest packet to make space for the newest packet if channel
                            // is full
                            if RECV_CHANNEL.try_send(packet).is_err() {
                                RECV_CHANNEL.try_receive();
                                RECV_CHANNEL.try_send(packet);
                            }
                        }
                        self.num_missed_events = 0;
                    } else if self.num_missed_events >= MAX_MISSED_EVENTS {
                        //log::info!("Switching to advertsing");
                        self.state = ConnectionState::Advertisement;
                    } else {
                        self.num_missed_events += 1;
                        self.prev_recv_time +=
                            TASK_TIMEOUT * NUM_CONNECTIONS as u32 * MAX_CONNECTION_EVENTS;
                        self.state = ConnectionState::ConnectedSend;
                    }
                }
                ConnectionState::ConnectedSend => {
                    let switch_to_rec_timeout = self.prev_recv_time
                        + TASK_TIMEOUT * NUM_CONNECTIONS as u32 * MAX_CONNECTION_EVENTS;
                    self.state = ConnectionState::ConnectedReceive;
                    let task = async {
                        let gurad_timeout = self
                            .prev_recv_time
                            .checked_add(
                                TASK_TIMEOUT
                                    * ((NUM_CONNECTIONS as u32 * MAX_CONNECTION_EVENTS) - 1),
                            )
                            .unwrap();
                        loop {
                            match select(Timer::at(gurad_timeout), SEND_CHANNEL.ready_to_receive())
                                .await
                            {
                                embassy_futures::select::Either::First(_) => {
                                    return;
                                }
                                embassy_futures::select::Either::Second(()) => {
                                    let next_period = get_next_time_period(self.prev_recv_time);
                                    if next_period >= gurad_timeout {
                                        break;
                                    } else {
                                        // Should be safe to unwrap as this run task is the only
                                        // receiver and we've reached this select state due to
                                        // ready_to_receive returning
                                        let mut packet = SEND_CHANNEL.try_receive().unwrap();
                                        let id = self.get_next_tx_id();
                                        let addr = self.rad.txaddress();
                                        packet.set_id(id);
                                        packet.set_type(PacketType::Data);

                                        Timer::at(next_period).await;
                                        for _ in 0..NUM_RETRIES {
                                            self.rad.send(&packet).await;

                                            let cond = |packet: &Packet| {
                                                packet.packet_type().unwrap() == PacketType::Ack
                                                    && packet.id() == id
                                                    && packet[0] == addr
                                            };
                                            if self
                                                .rad
                                                .receive_with_conditions(RECEIVE_TIMEOUT, cond)
                                                .await
                                                .is_some()
                                            {
                                                //log::info!("Packet sent and ack received!");
                                                self.num_missed_events = 0;
                                                return;
                                            }
                                        }
                                        //log::info!("Packet sent and no ack received!");
                                    }
                                }
                            }
                        }
                    };
                    join(task, Timer::at(switch_to_rec_timeout)).await;
                }
            }
        }
    }
}

fn get_next_time_period(time: Instant) -> Instant {
    let time_period = TASK_TIMEOUT * NUM_CONNECTIONS as u32;
    let periods = (time.elapsed().as_micros() / time_period.as_micros()) + 1;
    time + (time_period * periods as u32)
}
