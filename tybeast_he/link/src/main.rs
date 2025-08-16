use std::future;

use async_hid::{AsyncHidRead, AsyncHidWrite, Device, DeviceId, DeviceReader, DeviceWriter};
use async_hid::{DeviceInfo, HidBackend, HidResult};
use futures::future::select_all;
use futures::Stream;
use futures::StreamExt;
use tokio::join;
use tokio::sync::mpsc::{self, Receiver, Sender};

#[tokio::main]
async fn main() {
    env_logger::init();
    log::info!("hello");
    let (mut ltr_sender, mut ltr_rec) = mpsc::channel(10);
    let left_device_loop = async move {
        let backend = HidBackend::default();
        let dev = open_device(&backend, 0xFF69, 2, 0xa55, 0xa55).await;
        let mut writer = dev.open_writeable().await.unwrap();
        loop {
            let buf: [u8; 33] = ltr_rec.recv().await.unwrap();
            match writer.write_output_report(&buf).await {
                Ok(_) => {}
                Err(_) => {
                    let dev = open_device(&backend, 0xFF69, 2, 0xa55, 0xa55).await;
                    writer = dev.open_writeable().await.unwrap();
                }
            }
        }
    };

    let right_device_loop = async move {
        let backend = HidBackend::default();
        let dev = open_device(&backend, 0xFF69, 2, 0x727, 0x727).await;
        let mut reader = dev.open_readable().await.unwrap();
        loop {
            let mut buf = [0u8; 33];
            match reader.read_input_report(&mut buf[1..]).await {
                Ok(_) => {
                    ltr_sender.send(buf).await.unwrap();
                }
                Err(_) => {
                    let dev = open_device(&backend, 0xFF69, 2, 0x727, 0x727).await;
                    reader = dev.open_readable().await.unwrap();
                }
            }
        }
    };
    let l_handle = tokio::spawn(left_device_loop);
    right_device_loop.await;
    l_handle.await;
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
) -> ! {
    let backend = HidBackend::default();
    loop {
        let dev = open_device(&backend, usage_page, usage_id, vendor_id, product_id).await;
        let (mut reader, mut writer) = dev.open().await.unwrap();
        let read_loop = async {
            loop {
                let mut buf = [0u8; 33];
                match reader.read_input_report(&mut buf[1..]).await {
                    Ok(_) => {
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
