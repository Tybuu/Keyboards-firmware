#![cfg_attr(not(test), no_std)]
pub mod radio;

#[cfg(test)]
mod tests {
    use core::{
        cell::{Cell, RefCell},
        ptr::{self, null_mut},
    };
    use std::{
        collections::vec_deque::VecDeque,
        sync::mpsc::{self, Receiver, Sender},
    };

    use crate::radio::{
        Packet, PacketChannel, PacketType, Radio, RadioDriver, RadioTrigger, TimerDriver,
    };

    pub struct TimerTestDriver<'a> {
        finished: &'a RefCell<bool>,
    }

    impl<'a> TimerTestDriver<'a> {
        pub fn new(finished: &'a RefCell<bool>) -> Self {
            Self { finished }
        }
    }

    impl<'a> TimerDriver for TimerTestDriver<'a> {
        fn start_timer(&mut self) {}

        fn stop_timer(&mut self) {}

        fn clear_timer(&mut self) {}

        fn set_timer_duration(&mut self) {}

        fn timer_finished(&self) -> bool {
            *self.finished.borrow()
        }
    }
    #[derive(PartialEq, Eq)]
    enum RadioTestState {
        Disabled,
        Tx,
        Rx,
    }
    pub struct RadioTestDriver {
        state: RadioTestState,
        cur_packet: *mut u8,
        host_tx_queue: Receiver<Packet>,
        host_rx_queue: Sender<Packet>,
        pub rx_match: u8,
    }

    impl RadioTestDriver {
        pub fn new(host_tx_queue: Receiver<Packet>, host_rx_queue: Sender<Packet>) -> Self {
            Self {
                state: RadioTestState::Disabled,
                cur_packet: ptr::null_mut(),
                host_tx_queue,
                host_rx_queue,
                rx_match: 0,
            }
        }
    }

    impl RadioDriver for RadioTestDriver {
        fn is_disabled(&self) -> bool {
            self.state == RadioTestState::Disabled
        }

        fn disable(&mut self) {
            self.state = RadioTestState::Disabled;
        }

        fn set_packet(&mut self, packet: &mut crate::radio::Packet) {
            self.cur_packet = packet.buffer.as_mut_ptr();
        }

        fn crc_ok(&self) -> bool {
            true
        }

        fn start_tx(&mut self) {
            let mut packet = Packet::default();
            assert!(!self.cur_packet.is_null());
            unsafe {
                packet.set_len((*self.cur_packet - 2) as usize);
                packet.set_id(*self.cur_packet.offset(1));
                packet.buffer[2] = *self.cur_packet.offset(2);
                for i in 0..packet.len() {
                    packet[i] = *self.cur_packet.offset((i + 3).cast_signed());
                }
            }
            self.host_rx_queue.send(packet).unwrap();
        }

        fn start_rx(&mut self) {
            assert!(!self.cur_packet.is_null());
            if let Ok(packet) = self.host_tx_queue.try_recv() {
                unsafe {
                    *self.cur_packet = packet.buffer[0];
                    *self.cur_packet.offset(1) = packet.buffer[1];
                    *self.cur_packet.offset(2) = packet.buffer[2];
                    for i in 0..packet.len() {
                        *self.cur_packet.offset((i + 3).cast_signed()) = packet[i];
                    }
                }
            }
        }

        fn rx_match(&self) -> u8 {
            self.rx_match
        }
    }

    struct PacketTestChannel {
        pub queue: Sender<Packet>,
        pub done_queue: Sender<bool>,
    }

    impl PacketTestChannel {
        pub fn new(queue: Sender<Packet>, done_queue: Sender<bool>) -> Self {
            Self { queue, done_queue }
        }
    }

    impl PacketChannel for PacketTestChannel {
        fn send_packet(&self, packet: &Packet) {
            self.queue.send(*packet);
        }

        fn packet_tx_finished(&self) {
            self.done_queue.send(true);
        }
    }

    #[test]
    fn test_simple_send_with_ack() {
        let (mut device_receive_tx, device_receive_rx) = mpsc::channel();
        let (mut host_send_tx, host_send_rx) = mpsc::channel();
        let (mut host_receive_tx, host_receive_rx) = mpsc::channel();
        let (mut done_chan_tx, done_chan_rx) = mpsc::channel();
        let mut timer_done = RefCell::new(false);

        let mut rad_driver = RadioTestDriver::new(host_send_rx, host_receive_tx);
        let mut time_driver = TimerTestDriver::new(&timer_done);
        let mut chan = PacketTestChannel::new(device_receive_tx, done_chan_tx);
        let mut radio = Radio::new(rad_driver, time_driver, chan);

        let mut packet = Packet::default();
        packet.copy_from_slice(&[12, 44, 55, 66]);

        radio.start_radio_tx(&packet);
        let device_packet = host_receive_rx
            .try_recv()
            .expect("Host didn't receive packet!");

        packet.set_type(PacketType::Data);
        packet.set_id(0);
        assert!(device_packet == packet);

        let mut ack_packet = Packet::default();
        ack_packet.set_type(PacketType::Ack);
        ack_packet.set_id(device_packet.id());
        ack_packet.set_len(1);
        host_send_tx
            .send(ack_packet)
            .expect("unable to send ack packet");

        radio.step(RadioTrigger::Radio);
        radio.step(RadioTrigger::Radio);
        // Asssert firmly that we're in the disabled state, should just step through with no state changes
        radio.step(RadioTrigger::Radio);
        radio.step(RadioTrigger::Radio);
        radio.step(RadioTrigger::Radio);

        // No radio packets should've been resent
        assert!(host_receive_rx.try_recv().is_err());
    }

    #[test]
    fn test_send_with_missing_ack() {
        let (mut device_receive_tx, device_receive_rx) = mpsc::channel();
        let (mut host_send_tx, host_send_rx) = mpsc::channel();
        let (mut host_receive_tx, host_receive_rx) = mpsc::channel();
        let (mut done_chan_tx, done_chan_rx) = mpsc::channel();
        let mut timer_done = RefCell::new(false);

        let mut rad_driver = RadioTestDriver::new(host_send_rx, host_receive_tx);
        let mut time_driver = TimerTestDriver::new(&timer_done);
        let mut chan = PacketTestChannel::new(device_receive_tx, done_chan_tx);
        let mut radio = Radio::new(rad_driver, time_driver, chan);

        let mut ack_packet = Packet::default();
        ack_packet.set_type(PacketType::Ack);
        ack_packet.set_id(0);
        ack_packet.set_len(1);

        let mut packet = Packet::default();
        packet.copy_from_slice(&[12, 44, 55, 66]);

        radio.start_radio_tx(&packet);
        let device_packet = host_receive_rx
            .try_recv()
            .expect("Host didn't receive packet!");

        packet.set_type(PacketType::Data);
        packet.set_id(0);
        assert!(device_packet == packet);

        // (Tx -> TxAck)
        radio.step(RadioTrigger::Radio);

        *timer_done.borrow_mut() = true;
        // (TxAck -> Tx)
        radio.step(RadioTrigger::Timer);
        let device_packet = host_receive_rx
            .try_recv()
            .expect("Host didn't receive resent packet!");
        *timer_done.borrow_mut() = false;
        // Packet should be resent with id 0 with the same content as initial packet
        packet.set_type(PacketType::Data);
        packet.set_id(0);
        assert!(device_packet == packet);

        // Send an ack this iteration
        host_send_tx
            .send(ack_packet)
            .expect("unable to send ack packet");
        // (Tx -> TxAck)
        radio.step(RadioTrigger::Radio);
        // No radio packets should've been resent
        assert!(host_receive_rx.try_recv().is_err());
        // (TxAck -> Disabled)
        radio.step(RadioTrigger::Radio);

        // Verify that next packet sent is different
        let mut packet2 = Packet::default();
        packet2.copy_from_slice(&[95, 42, 18, 99]);

        radio.start_radio_tx(&packet2);

        // (Disabled -> Tx)
        let device_packet = host_receive_rx
            .try_recv()
            .expect("Host didn't receive packet!");
        packet2.set_type(PacketType::Data);
        packet2.set_id(1);
        assert!(device_packet == packet2);

        ack_packet.set_id(1);
        host_send_tx
            .send(ack_packet)
            .expect("Unable to send ack packet");
        // (Tx -> TxAck)
        radio.step(RadioTrigger::Radio);
        // (TxAck -> Disabled)
        radio.step(RadioTrigger::Radio);
        // Asssert firmly that we're in the disabled state, should just step through with no state changes.
        // If we aren't in disabled state, it should resend packet which will be detected below
        radio.step(RadioTrigger::Radio);

        // No radio packets should've been resent
        assert!(host_receive_rx.try_recv().is_err());
    }

    #[test]
    fn test_simple_receive() {
        let (mut device_receive_tx, device_receive_rx) = mpsc::channel();
        let (mut host_send_tx, host_send_rx) = mpsc::channel();
        let (mut host_receive_tx, host_receive_rx) = mpsc::channel();
        let (mut done_chan_tx, done_chan_rx) = mpsc::channel();
        let mut timer_done = RefCell::new(false);

        let mut rad_driver = RadioTestDriver::new(host_send_rx, host_receive_tx);
        let mut time_driver = TimerTestDriver::new(&timer_done);
        let mut chan = PacketTestChannel::new(device_receive_tx, done_chan_tx);
        let mut radio = Radio::new(rad_driver, time_driver, chan);

        let mut packet = Packet::default();
        packet.copy_from_slice(&[12, 44, 55, 66]);
        packet.set_id(0);
        packet.set_type(PacketType::Data);

        // (Disabled -> Rx)
        host_send_tx
            .send(packet)
            .expect("Unable to send host packet");
        radio.start_radio_rx();

        // (Rx -> RxAck)
        println!("To ack");
        radio.step(RadioTrigger::Radio);
        let ack_packet = host_receive_rx
            .try_recv()
            .expect("Didn't receive ack packet");
        assert!(ack_packet.id() == 0);
        assert!(ack_packet.is_packet_type(PacketType::Ack));
    }
}
