#![no_std]
#![no_main]

use core::sync::atomic::{AtomicBool, Ordering};

use defmt::info;
use embassy_executor::Spawner;
use embassy_futures::join::{join, join4};
use embassy_rp::adc::{self, Adc, Channel as AdcChannel, Config as AdcConfig};
use embassy_rp::flash::{Async, Flash};
use embassy_rp::gpio::{Level, Output, Pull};
use embassy_rp::peripherals::FLASH;
use embassy_rp::pio::Pio;
use embassy_rp::pio_programs::ws2812::{PioWs2812, PioWs2812Program, Rgb};
use embassy_rp::{bind_interrupts, peripherals, usb};

use embassy_rp::usb::Driver;
use embassy_sync::blocking_mutex::raw::{CriticalSectionRawMutex, ThreadModeRawMutex};
use embassy_sync::channel::Channel;
use embassy_sync::mutex::Mutex;
use embassy_time::Timer;
use embassy_usb::class::hid::{HidReaderWriter, HidWriter, State};
use embassy_usb::{Builder, Config, Handler};
use heapless::Vec;
use key_lib::com::{Com, KeyboardState};
use key_lib::descriptor::{BufferReport, KeyboardReportNKRO, MouseReport, SlaveReport};
use key_lib::keys::{Keys, SlaveKeys};
use key_lib::position::{HeSwitch, KeySensors, KeyState, SlavePosition};
use key_lib::report::Report;
use key_lib::storage::Storage;
use key_lib::NUM_KEYS;
use tybeast_ones_he::indicator::{Indicator, MasterIndicatorTask};
use tybeast_ones_he::sensors::MasterSensors;
use tybeast_ones_he::slave_com::{HidMaster, HidMasterTask};
use usbd_hid::descriptor::SerializedDescriptor;
use {defmt_rtt as _, panic_probe as _};

const FLASH_START: u32 = 1024 * 1024;
const FLASH_END: u32 = FLASH_START + 4096 * 5;
const FLASH_SIZE: usize = 2 * 1024 * 1024;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => usb::InterruptHandler<peripherals::USB>;
    ADC_IRQ_FIFO => adc::InterruptHandler;
    DMA_IRQ_0 => embassy_rp::dma::InterruptHandler<peripherals::DMA_CH0>, embassy_rp::dma::InterruptHandler<peripherals::DMA_CH1>;
    PIO0_IRQ_0 => embassy_rp::pio::InterruptHandler<peripherals::PIO0>;
});

#[embassy_executor::task]
async fn storage_task(storage: Storage<Flash<'static, FLASH, Async, FLASH_SIZE>>) {
    storage.run_storage().await;
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    info!("Device Started!");
    let p = embassy_rp::init(Default::default());
    // Create the driver, from the HAL.
    let driver = Driver::new(p.USB, Irqs);

    // Create embassy-usb Config
    let mut config = Config::new(0xa55, 0xa55);
    config.manufacturer = Some("Tybeast Corp.");
    config.product = Some("Tybeast Ones HE (Left)");
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
        hid_subclass: embassy_usb::class::hid::HidSubclass::No,
        hid_boot_protocol: embassy_usb::class::hid::HidBootProtocol::None,
        report_descriptor: KeyboardReportNKRO::desc(),
        request_handler: None,
        poll_ms: 1,
        max_packet_size: 32,
    };
    let slave_config = embassy_usb::class::hid::Config {
        hid_subclass: embassy_usb::class::hid::HidSubclass::No,
        hid_boot_protocol: embassy_usb::class::hid::HidBootProtocol::None,
        report_descriptor: SlaveReport::desc(),
        request_handler: None,
        poll_ms: 1,
        max_packet_size: 64,
    };
    let com_config = embassy_usb::class::hid::Config {
        hid_subclass: embassy_usb::class::hid::HidSubclass::No,
        hid_boot_protocol: embassy_usb::class::hid::HidBootProtocol::None,
        report_descriptor: BufferReport::desc(),
        request_handler: None,
        poll_ms: 1,
        max_packet_size: 64,
    };
    let mouse_config = embassy_usb::class::hid::Config {
        hid_subclass: embassy_usb::class::hid::HidSubclass::No,
        hid_boot_protocol: embassy_usb::class::hid::HidBootProtocol::None,
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

    let storage = Storage::init(
        Flash::<_, Async, FLASH_SIZE>::new(p.FLASH, p.DMA_CH0, Irqs),
        FLASH_START..FLASH_END,
    )
    .await;
    _spawner.spawn(storage_task(storage).unwrap());

    // Sel Pins
    let sel0 = Output::new(p.PIN_2, Level::Low);
    let sel1 = Output::new(p.PIN_1, Level::Low);
    let sel2 = Output::new(p.PIN_0, Level::Low);

    // Adc
    let adc = Adc::new(p.ADC, Irqs, AdcConfig::default());
    let a3 = AdcChannel::new_pin(p.PIN_26, Pull::None);
    let a2 = AdcChannel::new_pin(p.PIN_27, Pull::None);
    let a1 = AdcChannel::new_pin(p.PIN_28, Pull::None);
    let a0 = AdcChannel::new_pin(p.PIN_29, Pull::None);

    let mut order: [usize; NUM_KEYS / 2] = [
        7, 14, 2, 18, 5, 0, 3, 11, 6, 1, 9, 4, 15, 19, 10, 13, 17, 8, 12, 16, 20,
    ];
    find_order(&mut order);

    let hid_master_task = HidMasterTask::new();
    let mut key_sensors = MasterSensors::new(
        [a0, a1, a2, a3],
        [sel0, sel1, sel2],
        adc,
        hid_master_task.chan(),
        order,
    );
    let Pio {
        mut common, sm0, ..
    } = Pio::new(p.PIO0, Irqs);
    let program = PioWs2812Program::new(&mut common);
    let ws2812: PioWs2812<_, _, _, Rgb> =
        PioWs2812::with_color_order(&mut common, sm0, p.DMA_CH1, Irqs, p.PIN_17, &program);
    let indicator_task = MasterIndicatorTask::new(ws2812, hid_master_task.chan());

    let mut keys = Keys::default();
    keys.set_indicator(Indicator {});
    let _ = keys.load_keys_from_storage(0).await;

    let left_state = LeftState::new(keys);

    let mut com = Com::new(&left_state, com_reader, com_writer);
    let mut slave = SlaveKeys::new(hid_master_task.chan());
    let key_loop = async {
        let mut report = Report::new();
        let mut positions = [HeSwitch::DEFAULT; NUM_KEYS];
        positions[(NUM_KEYS / 2)..NUM_KEYS]
            .iter_mut()
            .for_each(|x| *x = HeSwitch::Slave(SlavePosition::DEFAULT));
        loop {
            key_sensors.update_positions(&mut positions).await;
            let is_slave = left_state.is_slave.load(Ordering::Acquire);
            if is_slave {
                slave.send_report(&positions[..(NUM_KEYS / 2)]).await;
            } else {
                let (key_rep, mouse_rep) =
                    report.generate_report(&left_state.keys, &positions).await;
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
            }
            Timer::after_micros(5).await;
        }
    };

    join4(
        usb_fut,
        join(com.com_loop(), indicator_task.run()),
        key_loop,
        hid_master_task.run(slave_hid),
    )
    .await;
}

struct MyDeviceHandler {
    configured: AtomicBool,
    indicator: Indicator,
}

impl MyDeviceHandler {
    fn new() -> Self {
        MyDeviceHandler {
            configured: AtomicBool::new(false),
            indicator: Indicator {},
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

    fn suspended(&mut self, suspended: bool) {
        self.indicator.suspend(suspended);
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

fn find_order(ary: &mut [usize]) {
    let mut new_ary = [0usize; NUM_KEYS / 2];
    for i in 0..ary.len() {
        for j in 0..ary.len() {
            if ary[j] == i {
                new_ary[i] = j;
            }
        }
    }
    ary.copy_from_slice(&new_ary);
}

struct LeftState {
    keys: Mutex<CriticalSectionRawMutex, Keys<Indicator>>,
    is_slave: AtomicBool,
}

impl LeftState {
    pub fn new(keys: Keys<Indicator>) -> Self {
        Self {
            keys: Mutex::new(keys),
            is_slave: AtomicBool::new(false),
        }
    }
}

impl KeyboardState for LeftState {
    async fn handle_request<'d, T: embassy_usb::driver::Driver<'d>>(
        &self,
        request: key_lib::com::HidRequest,
        reader: &mut key_lib::com::ContinuousReader<'d, T>,
        writer: &mut key_lib::com::ContinuousWriter<'d, T>,
    ) {
        match request {
            key_lib::com::HidRequest::UpdateKeys => {
                self.keys.handle_request(request, reader, writer).await
            }
            key_lib::com::HidRequest::KeyboardInfo => {
                self.keys.handle_request(request, reader, writer).await
            }
            key_lib::com::HidRequest::WriteToFlash => {
                self.keys.handle_request(request, reader, writer).await
            }
            key_lib::com::HidRequest::KeyboardMetaInfo => {
                self.keys.handle_request(request, reader, writer).await
            }
            key_lib::com::HidRequest::CurrentMode => {
                let is_slave = self.is_slave.load(Ordering::Acquire) as u8;
                writer.write(&[is_slave]).await;
                writer.flush().await;
            }
            key_lib::com::HidRequest::ToggleSlave => {
                let is_slave = self.is_slave.load(Ordering::Acquire);
                self.is_slave.store(!is_slave, Ordering::Release);
            }
        }
    }
}
