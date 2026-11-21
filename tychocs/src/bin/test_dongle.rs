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
    USBD => usb::InterruptHandler<peripherals::USBD>;
    CLOCK_POWER => usb::vbus_detect::InterruptHandler;
    RADIO  => radio::RadioInterruptHandler;
    TIMER0 => radio::RadioTimerInterrupt;
});

#[embassy_executor::task]
async fn logger_task(usbd: Peri<'static, peripherals::USBD>) {
    let driver = Driver::new(usbd, Irqs, HardwareVbusDetect::new(Irqs));
    embassy_usb_logger::run!(1024, log::LevelFilter::Info, driver);
}

#[embassy_executor::task]
async fn thread_task(mut rad: Radio<'static>) {
    let master_loop = async {
        let mut packet = Packet::default();
        packet.copy_from_slice(&[2, 4, 5, 7]);
        let mut count = 0;
        loop {
            rad.send_packet(&packet).await;
            log::info!("Sent Packet {}", count);
            Timer::after_millis(1000).await;
            count += 1;
        }
    };

    let heartbeat_loop = async {
        loop {
            log::info!("Heartbeat!");
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
    embassy_nrf::interrupt::USBD.set_priority(embassy_nrf::interrupt::Priority::P2);
    embassy_nrf::interrupt::CLOCK_POWER.set_priority(embassy_nrf::interrupt::Priority::P2);
    let mut radio = Radio::new(p.RADIO, p.TIMER0, Irqs, Irqs, Addresses::default());
    radio.set_tx_addresses(|w| w.set_txaddress(1));
    radio.set_rx_addresses(|w| {
        w.set_addr0(true);
    });
    let exectuor = THREAD_EXECUTOR.init_with(Executor::new);
    exectuor.run(|spawner| {
        spawner.spawn(thread_task(radio).unwrap());
        spawner.spawn(logger_task(p.USBD).unwrap());
    });
}
