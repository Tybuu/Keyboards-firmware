//! This example test the RP Pico on board LED.
//!
//! It does not work with the RP Pico W board. See wifi_blinky.rs.

#![no_std]
#![no_main]

use bruh78::radio::{self, Addresses, Packet, Radio};
use bruh78::sensors::Matrix;
use defmt::*;
use embassy_executor::Spawner;
use embassy_nrf::config::HfclkSource;
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pin, Pull};
use embassy_nrf::peripherals::USBD;
use embassy_nrf::{bind_interrupts, peripherals, usb};

use embassy_nrf::usb::Driver;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    RADIO => radio::InterruptHandler;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut config = embassy_nrf::config::Config::default();
    config.hfclk_source = HfclkSource::ExternalXtal;
    let p = embassy_nrf::init(config);

    let columns = [
        Output::new(p.P0_09, Level::Low, OutputDrive::Standard),
        Output::new(p.P0_10, Level::Low, OutputDrive::Standard),
        Output::new(p.P1_11, Level::Low, OutputDrive::Standard),
        Output::new(p.P1_15, Level::Low, OutputDrive::Standard),
        Output::new(p.P0_02, Level::Low, OutputDrive::Standard),
    ];

    let rows = [
        Input::new(p.P1_00, Pull::Down),
        Input::new(p.P0_11, Pull::Down),
        Input::new(p.P1_04, Pull::Down),
        Input::new(p.P1_06, Pull::Down),
    ];

    let mut matrix = Matrix::new(columns, rows);
    matrix.disable_debouncer(18..21);
    let addresses = Addresses::default();
    let mut radio = Radio::new(p.RADIO, Irqs, addresses);
    radio.set_tx_addresses(|w| w.set_txaddress(2));
    radio.set_rx_addresses(|w| w.set_addr0(true));
    let main_loop = async {
        let mut rep = 0;
        loop {
            matrix.update().await;
            let mut packet = Packet::default();
            packet.set_len(4);
            let new_rep = matrix.get_state();
            if new_rep != rep {
                rep = new_rep;
                packet.copy_from_slice(&rep.to_le_bytes());
                radio.send(&mut packet).await;
            }
        }
    };
    main_loop.await;
}
