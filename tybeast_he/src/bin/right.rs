//! This example test the RP Pico on board LED.
//!
//! It does not work with the RP Pico W board. See wifi_blinky.rs.

#![no_std]
#![no_main]

use core::sync::atomic::{AtomicBool, Ordering};
use core::time;

use defmt::info;
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_rp::adc::{self, Adc, Channel, Config as AdcConfig};
use embassy_rp::gpio::{Input, Pull};
use embassy_rp::{bind_interrupts, gpio, peripherals, usb};
use embassy_time::Timer;

use embassy_rp::usb::Driver;
use embassy_usb::class::hid::{HidReaderWriter, HidWriter, State};
use embassy_usb::{Builder, Config, Handler};
use gpio::{Level, Output};
use key_lib::descriptor::{BufferReport, SlaveReport};
use key_lib::keys::SlaveKeys;
use key_lib::position::{DefaultSwitch, HeSwitch, KeySensors};
use key_lib::NUM_KEYS;
use tybeast_ones_he::sensors::HallEffectSensors;
use usbd_hid::descriptor::SerializedDescriptor;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => usb::InterruptHandler<peripherals::USB>;
    ADC_IRQ_FIFO => adc::InterruptHandler;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("Device Started!");
    let p = embassy_rp::init(Default::default());
    // Create the driver, from the HAL.
    let driver = Driver::new(p.USB, Irqs);

    // Create embassy-usb Config
    let mut config = Config::new(0x727, 0x727);
    config.manufacturer = Some("Tybeast Corp.");
    config.product = Some("Tybeast Ones HE (Right)");
    config.max_power = 500;
    config.max_packet_size_0 = 64;
    config.composite_with_iads = true;
    config.device_class = 0xef;
    config.device_sub_class = 0x02;
    config.device_protocol = 0x01;

    // Create embassy-usb DeviceBuilder using the driver and config.
    // It needs some buffers for building the descriptors.
    let mut config_descriptor = [0; 256];
    let mut bos_descriptor = [0; 256];
    let mut msos_descriptor = [0; 256];
    let mut control_buf = [0; 64];
    let mut device_handler = MyDeviceHandler::new();

    let mut key_state = State::new();
    let mut com_state = State::new();

    let mut builder = Builder::new(
        driver,
        config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut msos_descriptor,
        &mut control_buf,
    );

    builder.handler(&mut device_handler);

    // Create classes on the builder.
    let key_config = embassy_usb::class::hid::Config {
        report_descriptor: SlaveReport::desc(),
        request_handler: None,
        poll_ms: 1,
        max_packet_size: 64,
    };
    let com_config = embassy_usb::class::hid::Config {
        report_descriptor: BufferReport::desc(),
        request_handler: None,
        poll_ms: 1,
        max_packet_size: 64,
    };

    let (_, mut key_writer) =
        HidReaderWriter::<_, 32, 32>::new(&mut builder, &mut key_state, key_config).split();
    let com_hid = HidReaderWriter::<_, 32, 32>::new(&mut builder, &mut com_state, com_config);

    let (mut c_reader, mut c_writer) = com_hid.split();

    // Build the builder.
    let mut usb = builder.build();
    let usb_fut = usb.run();

    // Sel Pins
    let sel0 = Output::new(p.PIN_0, Level::Low);
    let sel1 = Output::new(p.PIN_1, Level::Low);
    let sel2 = Output::new(p.PIN_2, Level::Low);

    // Adc
    let adc = Adc::new(p.ADC, Irqs, AdcConfig::default());
    let a3 = Channel::new_pin(p.PIN_26, Pull::None);
    let a2 = Channel::new_pin(p.PIN_27, Pull::None);
    let a1 = Channel::new_pin(p.PIN_28, Pull::None);
    let a0 = Channel::new_pin(p.PIN_29, Pull::None);

    let mut order: [usize; NUM_KEYS / 2] = [
        4, 5, 18, 2, 14, 7, 0, 9, 1, 6, 11, 3, 12, 17, 13, 10, 19, 15, 20, 16, 8,
    ];
    find_order(&mut order);

    let sensors = HallEffectSensors::new([a0, a1, a2, a3], [sel0, sel1, sel2], adc, order);

    let mut keys = SlaveKeys::<HeSwitch, _>::new(sensors);

    // Main keyboard loop
    let key_loop = async {
        loop {
            let rep = keys.generate_report().await;
            if let Some(rep) = rep {
                key_writer.write_serialize(rep).await.unwrap();
            }
            Timer::after_micros(5).await;
        }
    };
    join(usb_fut, key_loop).await;
}

struct MyDeviceHandler {
    configured: AtomicBool,
}

impl MyDeviceHandler {
    fn new() -> Self {
        MyDeviceHandler {
            configured: AtomicBool::new(false),
        }
    }
}

impl Handler for MyDeviceHandler {
    fn enabled(&mut self, enabled: bool) {
        self.configured.store(false, Ordering::Relaxed);
        if enabled {
            info!("Device enabled");
        } else {
            info!("Device disabled");
        }
    }

    fn reset(&mut self) {
        self.configured.store(false, Ordering::Relaxed);
        info!("Bus reset, the Vbus current limit is 100mA");
    }

    fn addressed(&mut self, addr: u8) {
        self.configured.store(false, Ordering::Relaxed);
        info!("USB address set to: {}", addr);
    }

    fn configured(&mut self, configured: bool) {
        self.configured.store(configured, Ordering::Relaxed);
        if configured {
            info!(
                "Device configured, it may now draw up to the configured current limit from Vbus."
            )
        } else {
            info!("Device is no longer configured, the Vbus current limit is 100mA.");
        }
    }
}

fn find_order(ary: &mut [usize]) {
    let mut new_ary = [0usize; 21 as usize];
    for i in 0..ary.len() {
        for j in 0..ary.len() {
            if ary[j] == i {
                new_ary[i] = j;
            }
        }
    }
    ary.copy_from_slice(&new_ary);
}
