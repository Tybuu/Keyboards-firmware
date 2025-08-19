#![no_std]
#![no_main]

use core::sync::atomic::{AtomicBool, Ordering};

use defmt::info;
use embassy_executor::Spawner;
use embassy_futures::join::{join, join4};
use embassy_rp::gpio::Output;
use embassy_rp::{bind_interrupts, peripherals, usb};

use embassy_rp::usb::Driver;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_usb::class::hid::{HidReaderWriter, HidWriter, State};
use embassy_usb::{Builder, Config, Handler};
use key_lib::descriptor::{BufferReport, KeyboardReportNKRO, MouseReport, SlaveReport};
use usbd_hid::descriptor::SerializedDescriptor;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => usb::InterruptHandler<peripherals::USB>;
});

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("Device Started!");
    let p = embassy_rp::init(Default::default());
    // Create the driver, from the HAL.
    let driver = Driver::new(p.USB, Irqs);

    // Create embassy-usb Config
    let mut config = Config::new(0xa56, 0xa56);
    config.manufacturer = Some("Tybeast Corp.");
    config.product = Some("Tybeast Test 2");
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

    let mut key_state = State::new();
    let mut slave_state = State::new();
    let mut mouse_state = State::new();
    let mut com_state = State::new();
    let mut device_handler =
        MyDeviceHandler::new(Output::new(p.PIN_25, embassy_rp::gpio::Level::Low));

    let mut builder = Builder::new(
        driver,
        config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut msos_descriptor,
        &mut control_buf,
    );

    // Create classes on the builder.
    let key_config = embassy_usb::class::hid::Config {
        report_descriptor: KeyboardReportNKRO::desc(),
        request_handler: None,
        poll_ms: 1,
        max_packet_size: 32,
    };
    let slave_config = embassy_usb::class::hid::Config {
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
    let mouse_config = embassy_usb::class::hid::Config {
        report_descriptor: MouseReport::desc(),
        request_handler: None,
        poll_ms: 1,
        max_packet_size: 5,
    };
    builder.handler(&mut device_handler);
    let mut key_writer = HidWriter::<_, 29>::new(&mut builder, &mut key_state, key_config);
    let mut slave_hid =
        HidReaderWriter::<_, 32, 32>::new(&mut builder, &mut slave_state, slave_config);
    let (com_reader, com_writer) =
        HidReaderWriter::<_, 32, 32>::new(&mut builder, &mut com_state, com_config).split();
    let mut mouse_writer = HidWriter::<_, 5>::new(&mut builder, &mut mouse_state, mouse_config);

    // Build the builder.
    let mut usb = builder.build();
    let usb_fut = usb.run();

    usb_fut.await;
}

struct MyDeviceHandler<'d> {
    configured: AtomicBool,
    indicator: Mutex<CriticalSectionRawMutex, Output<'d>>,
}

impl<'d> MyDeviceHandler<'d> {
    fn new(output: Output<'d>) -> Self {
        MyDeviceHandler {
            configured: AtomicBool::new(false),
            indicator: Mutex::new(output),
        }
    }
}

impl<'d> Handler for MyDeviceHandler<'d> {
    fn enabled(&mut self, enabled: bool) {
        self.configured.store(false, Ordering::Relaxed);
        if enabled {
            info!("Device enabled");
        } else {
            info!("Device disabled");
        }
    }

    fn suspended(&mut self, suspended: bool) {
        if let Ok(mut pin) = self.indicator.try_lock() {
            if suspended {
                pin.set_low();
            } else {
                pin.set_high();
            }
        }
        if suspended {
            info!("Host computer is suspended!");
        } else {
            info!("Host computer isn't suspended!");
        }
    }

    fn reset(&mut self) {
        self.configured.store(false, Ordering::Relaxed);
        info!("Bus reset, the Vbus current limit is 500mA");
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
