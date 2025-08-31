use std::future;

use async_hid::{AsyncHidRead, AsyncHidWrite, Device, DeviceId, DeviceReader, DeviceWriter};
use async_hid::{DeviceInfo, HidBackend, HidResult};
use futures::StreamExt;
use tokio::join;
use tokio::sync::mpsc::{self, Receiver, Sender};

const USAGE_PAGE: u16 = 0xFF69;
const USAGE: u16 = 0x2;
#[tokio::main]
async fn main() {
    env_logger::init();
    log::info!("hello");
    let (l_sender, l_rec) = mpsc::channel(10);
    let (r_sender, r_rec) = mpsc::channel(10);
    let l_future = run_device(r_rec, l_sender, USAGE_PAGE, USAGE, 0xa55, 0xa55);
    let r_future = run_device(l_rec, r_sender, USAGE_PAGE, USAGE, 0x727, 0x727);
    join!(l_future, r_future);
}

async fn open_device(
    backend: &HidBackend,
    usage_page: u16,
    usage_id: u16,
    vendor_id: u16,
    product_id: u16,
) -> Device {
    log::info!(
        "Finding Device {:x}:{:x} | Usage Page: {:x} | Usage: {}",
        vendor_id,
        product_id,
        usage_page,
        usage_id
    );
    // Initial Search
    let mut devices = backend.enumerate().await.unwrap();
    while let Some(new_dev) = devices.next().await {
        if new_dev.matches(usage_page, usage_id, vendor_id, product_id) {
            log::info!(
                "Connected to Device {:x}:{:x} | Usage Page: {:x} | Usage: {}",
                vendor_id,
                product_id,
                usage_page,
                usage_id
            );
            return new_dev;
        }
    }
    loop {
        let mut watch = backend.watch().unwrap();
        while let Some(event) = watch.next().await {
            match event {
                async_hid::DeviceEvent::Connected(device_id) => {
                    log::info!("Device connected! {:?}", device_id);
                    let res = backend
                        .query_devices(&device_id)
                        .await
                        .unwrap()
                        .find(|dev| dev.matches(usage_page, usage_id, vendor_id, product_id));
                    if let Some(dev) = res {
                        log::info!(
                            "Connected to Device {:x}:{:x} | Usage Page: {:x} | Usage: {}",
                            vendor_id,
                            product_id,
                            usage_page,
                            usage_id
                        );
                        return dev;
                    }
                }
                async_hid::DeviceEvent::Disconnected(device_id) => {}
            }
        }
    }
}

type BufferData = [u8; 33];
pub async fn run_device(
    mut rec: Receiver<BufferData>,
    sender: Sender<BufferData>,
    usage_page: u16,
    usage_id: u16,
    vendor_id: u16,
    product_id: u16,
) {
    let backend = HidBackend::default();
    loop {
        let dev = open_device(&backend, usage_page, usage_id, vendor_id, product_id).await;
        let (mut reader, mut writer) = dev.open().await.unwrap();
        let read_loop = async {
            loop {
                let mut buf = [0u8; 33];
                match reader.read_input_report(&mut buf[1..]).await {
                    Ok(_) => {
                        log::info!("From {:x}:{:x} | {:?}", vendor_id, product_id, buf);
                        sender.send(buf).await.unwrap();
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
        };
        let write_loop = async {
            loop {
                let buf = rec.recv().await.unwrap();
                match writer.write_output_report(&buf).await {
                    Ok(_) => {}
                    Err(_) => {
                        break;
                    }
                }
            }
        };
        tokio::select! {
            _ = read_loop => {},
            _ = write_loop => {}
        }
        log::info!("Device {:x}:{:x} closed", vendor_id, product_id);
    }
}
