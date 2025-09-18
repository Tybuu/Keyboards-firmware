use defmt::info;
use embassy_futures::select::select;
use embassy_nrf::{
    interrupt,
    pac::radio::regs::{Rxaddresses, Txaddress},
    Peri,
};

use crate::radio::{
    inner_radio::Radio, packet::PacketType, Addresses, InterruptHandler, RECV_CHANNEL, SEND_CHANNEL,
};

pub struct CRadio<'d> {
    rad: Radio<'d>,
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
            let recv_task = async {
                loop {
                    let packet = self.rad.receive().await;
                    if packet.packet_type().unwrap() == PacketType::Data {
                        if RECV_CHANNEL.try_send(packet).is_err() {
                            RECV_CHANNEL.try_receive();
                            RECV_CHANNEL.try_send(packet);
                        }
                    } else {
                        info!(
                            "Received invalid packet from {} with data type: {:?}",
                            packet.addr,
                            packet.packet_type().unwrap()
                        );
                    }
                }
            };
            match select(SEND_CHANNEL.receive(), recv_task).await {
                embassy_futures::select::Either::First(mut packet) => {
                    packet.set_type(PacketType::Data);
                    self.rad.send(&packet).await;
                }
                embassy_futures::select::Either::Second(_) => {}
            }
        }
    }
}
