#![no_std]
#![no_main]

use core::sync::atomic::{AtomicBool, Ordering};

use bruh78::{
    key_config::set_keys,
    radio::{self, Addresses, Radio},
    sensors::DongleSensors,
};
use defmt::{info, *};
use embassy_executor::Spawner;
use embassy_futures::join::{join, join3, join4};
use embassy_nrf::{
    bind_interrupts,
    config::HfclkSource,
    peripherals::{self, USBD},
    qspi::Qspi,
    usb::{self, vbus_detect::HardwareVbusDetect, Driver},
};

use defmt_rtt as _; // global logger
use embassy_nrf as _;
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, mutex::Mutex};
use embassy_time::Timer;
use embassy_usb::{
    class::hid::{HidReaderWriter, HidWriter, State},
    Builder, Handler,
};
use key_lib::{
    com::Com,
    descriptor::{BufferReport, KeyboardReportNKRO, MouseReport},
    keys::{ConfigIndicator, Indicate, Keys},
    position::DefaultSwitch,
    report::Report,
    storage::Storage,
};
// time driver
use panic_probe as _;
use sequential_storage::cache::NoCache;
use static_cell::StaticCell;
use usbd_hid::descriptor::SerializedDescriptor;

static KEYS: Mutex<ThreadModeRawMutex, Keys<Indicator>> = Mutex::new(Keys::default());

static CACHE: StaticCell<NoCache> = StaticCell::new();

bind_interrupts!(struct Irqs {
    USBD => usb::InterruptHandler<peripherals::USBD>;
    CLOCK_POWER => usb::vbus_detect::InterruptHandler;
    RADIO  => radio::InterruptHandler;
    QSPI => embassy_nrf::qspi::InterruptHandler<peripherals::QSPI>;
});

#[embassy_executor::task]
async fn storage_task(storage: Storage<Qspi<'static, peripherals::QSPI>, NoCache>) {
    storage.run_storage().await;
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let mut nrf_config = embassy_nrf::config::Config::default();
    nrf_config.hfclk_source = HfclkSource::ExternalXtal;
    let p = embassy_nrf::init(nrf_config);

    let driver = Driver::new(p.USBD, Irqs, HardwareVbusDetect::new(Irqs));

    // Create embassy-usb Config
    let mut config = embassy_usb::Config::new(0xa55, 0xa44);
    config.manufacturer = Some("Tybeast Corp.");
    config.product = Some("TyDongle");
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
    let mut mouse_state = State::new();
    let mut com_state = State::new();
    let mut device_handler = MyDeviceHandler::new();

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
        max_packet_size: 29,
    };
    let com_config = embassy_usb::class::hid::Config {
        report_descriptor: BufferReport::desc(),
        request_handler: None,
        poll_ms: 1,
        max_packet_size: 32,
    };
    let mouse_config = embassy_usb::class::hid::Config {
        report_descriptor: MouseReport::desc(),
        request_handler: None,
        poll_ms: 1,
        max_packet_size: 5,
    };
    builder.handler(&mut device_handler);
    let mut key_writer = HidWriter::<_, 29>::new(&mut builder, &mut key_state, key_config);
    let (com_reader, com_writer) =
        HidReaderWriter::<_, 32, 32>::new(&mut builder, &mut com_state, com_config).split();
    let mut mouse_writer = HidWriter::<_, 5>::new(&mut builder, &mut mouse_state, mouse_config);

    // Build the builder.
    let mut usb = builder.build();
    let usb_fut = usb.run();

    let cache = CACHE.init_with(NoCache::new);
    let mut qspi_config = embassy_nrf::qspi::Config::default();
    qspi_config.sck_delay = 5;
    qspi_config.read_opcode = embassy_nrf::qspi::ReadOpcode::READ4O;
    qspi_config.write_opcode = embassy_nrf::qspi::WriteOpcode::PP4O;
    qspi_config.frequency = embassy_nrf::qspi::Frequency::M32;
    qspi_config.address_mode = embassy_nrf::qspi::AddressMode::_24BIT;
    qspi_config.capacity = 0x200000;

    // let qspi_flash = Qspi::new(
    //     p.QSPI,
    //     Irqs,
    //     p.P0_21,
    //     p.P0_25,
    //     p.P0_20,
    //     p.P0_24,
    //     p.P0_22,
    //     p.P0_23,
    //     qspi_config,
    // );
    //
    // let storage = Storage::init(qspi_flash, 0..(4096 * 5), cache).await;
    // spawner.spawn(storage_task(storage)).unwrap();

    let addresses = Addresses::default();

    let mut radio = Radio::new(p.RADIO, Irqs, addresses);
    radio.set_tx_addresses(|w| w.set_txaddress(0));
    radio.set_rx_addresses(|w| {
        w.set_addr1(true);
        w.set_addr2(true);
    });

    let sensors = DongleSensors {};
    let mut report: Report<_, DefaultSwitch> = Report::new(sensors);

    let mut keys = KEYS.lock().await;
    set_keys(&mut keys);
    // keys.load_keys_from_storage(0).await;
    drop(keys);

    let mut com = Com::new(&KEYS, com_reader, com_writer);
    let key_loop = async {
        loop {
            let (key_rep, mouse_rep);
            {
                (key_rep, mouse_rep) = report.generate_report(&KEYS).await;
            }
            let key_task = async {
                if let Some(rep) = key_rep {
                    info!("Writing key report!");
                    key_writer.write_serialize(rep).await.unwrap();
                }
            };
            let mouse_task = async {
                if let Some(rep) = mouse_rep {
                    mouse_writer.write_serialize(rep).await.unwrap();
                }
            };
            join(key_task, mouse_task).await;
            Timer::after_micros(5).await;
        }
    };
    join4(usb_fut, key_loop, com.com_loop(), radio.run_receive()).await;
}

struct Indicator {}

impl ConfigIndicator for Indicator {
    async fn indicate_config(&self, config_num: Indicate) {}
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
