#![no_std]
#![no_main]

use core::{mem, ops::Deref};

use bruh78::radio::{self, Addresses, Packet, Radio};
use cortex_m_rt::entry;
use defmt::{info, *};
use embassy_executor::{Executor, InterruptExecutor, Spawner};
use embassy_futures::join::join;
use embassy_nrf::{
    bind_interrupts,
    config::HfclkSource,
    gpio::Output,
    interrupt,
    interrupt::InterruptExt,
    peripherals::{self, USBD},
    usb::{self, vbus_detect::HardwareVbusDetect, Driver},
    Peri,
};

use defmt_rtt as _; // global logger
use embassy_nrf as _;
use embassy_time::Timer;
// time driver
use panic_probe as _;
use static_cell::StaticCell;

static THREAD_EXECUTOR: StaticCell<Executor> = StaticCell::new();

bind_interrupts!(struct Irqs {
    RADIO  => radio::RadioInterruptHandler;
    TIMER0 => radio::RadioTimerInterrupt;
});

#[embassy_executor::task]
async fn thread_task(mut rad: Radio<'static>) {
    let master_loop = async {
        loop {
            let packet = rad.receive_packet().await;
            info!("pakcet received");
        }
    };

    let heartbeat_loop = async {
        loop {
            info!("Heartbeat!");
            Timer::after_secs(2).await;
        }
    };
    join(master_loop, heartbeat_loop).await;
}

#[entry]
fn main() -> ! {
    log::info!("Hello World!");

    let mut nrf_config = embassy_nrf::config::Config::default();
    nrf_config.hfclk_source = HfclkSource::ExternalXtal;
    let p = embassy_nrf::init(nrf_config);

    embassy_nrf::interrupt::RADIO.set_priority(embassy_nrf::interrupt::Priority::P0);
    let mut radio = Radio::new(p.RADIO, p.TIMER0, Irqs, Irqs, Addresses::default());
    radio.set_tx_addresses(|w| w.set_txaddress(0));
    radio.set_rx_addresses(|w| {
        w.set_addr1(true);
    });
    let exectuor = THREAD_EXECUTOR.init_with(Executor::new);
    exectuor.run(|spawner| {
        spawner.spawn(thread_task(radio).unwrap());
    });
}
