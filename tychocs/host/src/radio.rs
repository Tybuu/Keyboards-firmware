use core::{
    fmt::Debug,
    sync::{self, atomic::compiler_fence},
};

const BUFFER_SIZE: usize = 32;
const META_SIZE: usize = 3;

pub trait RadioDriver {
    fn is_disabled(&self) -> bool;
    fn disable(&mut self);
    fn set_packet(&mut self, packet: &mut Packet);
    fn crc_ok(&self) -> bool;
    fn start_tx(&mut self);
    fn start_rx(&mut self);
    fn rx_match(&self) -> u8;
}

pub trait TimerDriver {
    fn start_timer(&mut self);
    fn stop_timer(&mut self);
    fn clear_timer(&mut self);
    fn set_timer_duration(&mut self);
    fn timer_finished(&self) -> bool;
}

pub trait PacketChannel {
    fn send_packet(&self, packet: &Packet);
    fn packet_tx_finished(&self);
}

#[derive(Default, Debug, PartialEq, Eq)]
enum RadioState {
    #[default]
    Disabled,
    Tx,
    TxAck,
    Rx,
    RxAck,
}

pub enum RadioTrigger {
    Radio,
    Timer,
}

pub struct Radio<R: RadioDriver, T: TimerDriver, C: PacketChannel> {
    radio: R,
    timer: T,
    tx_id: u8,
    rx_id: [u8; 8],
    ack_packet: Packet,
    packet: Packet,
    state: RadioState,
    chan: C,
}

impl<R: RadioDriver, T: TimerDriver, C: PacketChannel> Debug for Radio<R, T, C> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Radio")
            .field("tx_id", &self.tx_id)
            .field("rx_id", &self.rx_id)
            .field("ack_packet", &self.ack_packet)
            .field("packet", &self.packet)
            .field("state", &self.state)
            .finish()
    }
}

impl<R: RadioDriver, T: TimerDriver, C: PacketChannel> Radio<R, T, C> {
    pub fn new(radio: R, timer: T, chan: C) -> Self {
        Self {
            radio,
            timer,
            tx_id: 0,
            rx_id: [128; 8],
            ack_packet: Packet::default(),
            packet: Packet::default(),
            state: RadioState::default(),
            chan,
        }
    }

    pub fn start_radio_tx(&mut self, packet: &Packet) {
        assert!(self.state == RadioState::Disabled);
        self.packet = *packet;
        self.packet.set_id(self.tx_id);
        self.packet.set_type(PacketType::Data);
        self.state = RadioState::Tx;
        self.radio.set_packet(&mut self.packet);
        compiler_fence(core::sync::atomic::Ordering::Release);
        self.radio.start_tx();
    }

    pub fn start_radio_rx(&mut self) {
        assert!(self.state == RadioState::Disabled);
        self.radio.set_packet(&mut self.packet);
        self.state = RadioState::Rx;
        compiler_fence(core::sync::atomic::Ordering::Release);
        self.radio.start_rx();
    }

    fn handle_radio_event(&mut self) {
        match self.state {
            RadioState::Disabled => {}
            RadioState::Tx => {
                if self.radio.is_disabled() {
                    self.radio.set_packet(&mut self.ack_packet);
                    self.state = RadioState::TxAck;
                    compiler_fence(core::sync::atomic::Ordering::Release);
                    self.timer.start_timer();
                    self.radio.start_rx();
                }
            }
            RadioState::TxAck => {
                if self.radio.is_disabled() {
                    if self.radio.crc_ok() {
                        if self.ack_packet.is_packet_type(PacketType::Ack)
                            && self.ack_packet.id() == self.tx_id
                        {
                            self.tx_id = self.tx_id.overflowing_add(1).0;
                            self.state = RadioState::Disabled;
                            self.timer.stop_timer();
                            self.chan.packet_tx_finished();
                        } else {
                            self.radio.start_rx();
                        }
                    } else {
                        self.state = RadioState::Tx;
                        self.radio.set_packet(&mut self.packet);
                        compiler_fence(core::sync::atomic::Ordering::Release);
                        self.timer.stop_timer();
                        self.radio.start_tx();
                    }
                }
            }
            RadioState::Rx => {
                if self.radio.is_disabled() {
                    if self.radio.crc_ok() {
                        #[cfg(test)]
                        dbg!(self.packet);
                        if self.packet.is_packet_type(PacketType::Data) {
                            self.state = RadioState::RxAck;
                            self.ack_packet.set_len(1);
                            self.ack_packet.set_type(PacketType::Ack);
                            self.ack_packet.set_id(self.packet.id());
                            self.radio.set_packet(&mut self.ack_packet);
                            compiler_fence(core::sync::atomic::Ordering::Release);
                            self.radio.start_tx();
                        } else {
                            self.radio.start_rx();
                        }
                    } else {
                        self.radio.start_rx();
                    }
                }
            }
            RadioState::RxAck => {
                if self.radio.is_disabled() {
                    let addr = self.radio.rx_match();
                    self.state = RadioState::Rx;
                    if self.packet.id() != self.rx_id[addr as usize] {
                        self.rx_id[addr as usize] = self.packet.id();
                        self.packet.addr = addr;
                        self.chan.send_packet(&self.packet);
                    }
                    self.radio.set_packet(&mut self.packet);
                    self.radio.start_rx();
                }
            }
        }
    }

    fn handle_timer_event(&mut self) {
        match self.state {
            RadioState::Disabled => {
                self.timer.timer_finished();
                self.timer.stop_timer();
                self.timer.clear_timer();
            }
            RadioState::Tx => {
                self.timer.timer_finished();
                self.timer.stop_timer();
                self.timer.clear_timer();
            }
            RadioState::TxAck => {
                if self.timer.timer_finished() {
                    self.timer.stop_timer();
                    self.timer.clear_timer();
                    self.state = RadioState::Disabled;
                    self.radio.disable();
                    self.state = RadioState::Tx;
                    self.radio.set_packet(&mut self.packet);
                    compiler_fence(core::sync::atomic::Ordering::Release);
                    self.radio.start_tx();
                }
            }
            RadioState::Rx => todo!(),
            RadioState::RxAck => todo!(),
        }
    }

    pub fn step(&mut self, trigger: RadioTrigger) {
        match trigger {
            RadioTrigger::Radio => self.handle_radio_event(),
            RadioTrigger::Timer => self.handle_timer_event(),
        }
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
    pub(crate) buffer: [u8; BUFFER_SIZE + META_SIZE],
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
        // Subtract META_SIZE by 1 for len as len field doesn't count the len byte in the nrf52840
        // radio peripheral
        self.buffer[Self::LEN_INDEX] as usize - (META_SIZE - 1)
    }

    pub fn set_len(&mut self, len: usize) {
        self.buffer[Self::LEN_INDEX] = (META_SIZE - 1) as u8 + len as u8;
    }

    pub fn clear(&mut self) {
        self.set_len(0);
    }

    pub fn id(&self) -> u8 {
        self.buffer[Self::ID_INDEX]
    }

    pub fn set_id(&mut self, id: u8) {
        self.buffer[Self::ID_INDEX] = id;
    }

    pub fn is_packet_type(&self, packet_type: PacketType) -> bool {
        self.packet_type().is_ok_and(|x| x == packet_type)
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

#[test]
fn test_empty_packet() {
    let mut packet = Packet::default();
    assert!(packet.is_empty());
    assert!(packet.len() == 0);
    packet.copy_from_slice(&[1, 2, 3]);
    assert!(!packet.is_empty());
    packet.clear();
    assert!(packet.is_empty());
}

#[test]
fn test_packet_copy_slice() {
    let copy = [2, 3, 4, 5];
    let mut packet = Packet::default();
    packet.copy_from_slice(&copy);
    assert!(packet.len() == copy.len());
    for i in 0..packet.len() {
        assert!(packet[i] == copy[i]);
    }
}
