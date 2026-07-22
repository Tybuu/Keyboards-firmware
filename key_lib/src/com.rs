use core::ops::{Deref, DerefMut};

use defmt::{error, info};
use embassy_sync::blocking_mutex::raw::RawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::Instant;
use embassy_usb::class::hid::{HidReader, HidWriter};
use embassy_usb::driver::Driver;

use crate::keys::{ConfigIndicator, Keys};

use crate::descriptor::BufferReport;
use crate::{IS_SPLIT, NUM_CONFIGS, NUM_KEYS, NUM_LAYERS};

const BUFFER_SIZE: usize = 32;

pub struct ContinuousWriter<'d, T: Driver<'d>> {
    writer: HidWriter<'d, T, 32>,
    index: usize,
    buffer: BufferReport,
}

impl<'d, T: Driver<'d>> ContinuousWriter<'d, T> {
    pub fn new(writer: HidWriter<'d, T, 32>) -> Self {
        Self {
            writer,
            index: 0,
            buffer: BufferReport {
                input: [0; 32],
                output: [0; 32],
            },
        }
    }

    pub async fn write(&mut self, buf: &[u8]) {
        let mut buf_index = 0;
        while buf_index < buf.len() {
            let buf_end = (buf_index + (BUFFER_SIZE - self.index)).min(buf.len());
            let write_len = buf_end - buf_index;
            let rep_end = self.index + write_len;
            self.buffer.input[self.index..rep_end].copy_from_slice(&buf[buf_index..buf_end]);
            buf_index = buf_end;
            if rep_end == 32 {
                self.writer.write_serialize(&self.buffer).await.unwrap();
                self.index = 0;
            } else {
                self.index = rep_end;
            }
        }
    }

    pub async fn flush(&mut self) {
        if self.index != 0 {
            self.buffer.input[self.index..].fill(0);
            self.writer.write_serialize(&self.buffer).await.unwrap();
            self.index = 0;
        }
    }
}

pub struct ContinuousReader<'d, T: Driver<'d>> {
    reader: HidReader<'d, T, 32>,
    index: usize,
    buffer_len: usize,
    buffer: [u8; 32],
}

impl<'d, T: Driver<'d>> ContinuousReader<'d, T> {
    pub fn new(reader: HidReader<'d, T, 32>) -> Self {
        Self {
            reader,
            index: 0,
            buffer_len: 0,
            buffer: [0u8; BUFFER_SIZE],
        }
    }

    pub fn flush(&mut self) {
        self.index = 0;
    }

    pub async fn pop(&mut self) -> u8 {
        if self.index == 0 {
            self.buffer_len = self.reader.read(&mut self.buffer).await.unwrap();
        }

        let val = self.buffer[self.index];

        self.index += 1;
        if self.index == self.buffer_len {
            self.index = 0;
        }
        val
    }

    pub async fn pop_slice(&mut self, buf: &mut [u8]) {
        let mut buf_index = 0;
        while buf_index < buf.len() {
            if self.index == 0 {
                self.buffer_len = self.reader.read(&mut self.buffer).await.unwrap();
            }
            let buf_end = (buf_index + (self.buffer_len - self.index)).min(buf.len());
            let write_len = buf_end - buf_index;

            let rep_end = self.index + write_len;
            buf[buf_index..buf_end].copy_from_slice(&self.buffer[self.index..rep_end]);

            buf_index = buf_end;
            if rep_end == self.buffer_len {
                self.index = 0;
            } else {
                self.index = rep_end;
            }
        }
    }
}

#[repr(u8)]
pub enum HidRequest {
    UpdateKeys = 0,
    KeyboardInfo = 1,
    WriteToFlash = 2,
    KeyboardMetaInfo = 3,
    CurrentMode = 4,
    ToggleSlave = 5,
}

impl From<u8> for HidRequest {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::UpdateKeys,
            1 => Self::KeyboardInfo,
            2 => Self::WriteToFlash,
            3 => Self::KeyboardMetaInfo,
            4 => Self::CurrentMode,
            5 => Self::ToggleSlave,
            _ => todo!(),
        }
    }
}

pub trait KeyboardState {
    fn handle_request<'d, T: Driver<'d>>(
        &self,
        request: HidRequest,
        reader: &mut ContinuousReader<'d, T>,
        writer: &mut ContinuousWriter<'d, T>,
    ) -> impl Future<Output = ()>;
}

impl<M: RawMutex, I: ConfigIndicator> KeyboardState for Mutex<M, Keys<I>> {
    async fn handle_request<'d, T: Driver<'d>>(
        &self,
        hid_request: HidRequest,
        reader: &mut ContinuousReader<'d, T>,
        writer: &mut ContinuousWriter<'d, T>,
    ) {
        match hid_request {
            HidRequest::UpdateKeys => {
                let config_num = reader.pop().await as usize;
                let mut keys = self.lock().await;
                match keys.load_keys_from_com(reader, config_num).await {
                    Ok(_) => {
                        info!("Finished Receiving bytes");
                    }
                    Err(_) => {
                        error!("Unable to read from com to deserialzie keyboard config");
                        let _ = keys.load_keys_from_storage(0).await;
                    }
                }
                drop(keys);
            }
            HidRequest::KeyboardInfo => {
                info!("Sending keyboard config!");
                let mut default_keys = Keys::default();
                for config_num in 0..NUM_CONFIGS {
                    let start = Instant::now();
                    let lock = self.lock().await;
                    let keys = if lock.config_num == config_num {
                        lock.deref()
                    } else {
                        drop(lock);
                        let _ = default_keys.load_keys_from_storage(config_num).await;
                        &default_keys
                    };
                    let load_time = Instant::now();
                    keys.write_keys_to_com(writer).await;
                    let write_time = Instant::now();
                    info!(
                        "Writing to com config {} | Write Time : {}ms | Load Time : {}ms",
                        config_num,
                        (write_time - load_time).as_millis(),
                        (load_time - start).as_millis(),
                    );
                }
                writer.flush().await;
                info!("Finished sending keyboard config!");
            }
            HidRequest::WriteToFlash => {
                let mut default_keys = Keys::default();
                for config_num in 0..NUM_CONFIGS {
                    let mut lock = self.lock().await;
                    let keys = if lock.config_num == config_num {
                        lock.deref_mut()
                    } else {
                        drop(lock);
                        &mut default_keys
                    };
                    keys.load_keys_from_com(reader, config_num).await.unwrap();
                    info!("Succesfully loaded config {}!", config_num);
                    keys.write_keys_to_storage(config_num).await;
                }
                info!("Finished writing config to storage");
            }
            HidRequest::KeyboardMetaInfo => {
                info!("Requested Keyboard meta info!");
                writer
                    .write(&[
                        NUM_CONFIGS as u8,
                        NUM_KEYS as u8,
                        NUM_LAYERS as u8,
                        IS_SPLIT as u8,
                    ])
                    .await;
                writer.flush().await;
            }
            HidRequest::CurrentMode => {
                writer.write(&[0]).await;
            }
            HidRequest::ToggleSlave => {}
        }
    }
}
pub struct Com<'a, 'd, T: Driver<'d>, K: KeyboardState> {
    keys: &'a K,
    reader: ContinuousReader<'d, T>,
    writer: ContinuousWriter<'d, T>,
}

impl<'a, 'd, K: KeyboardState, T: Driver<'d>> Com<'a, 'd, T, K> {
    pub fn new(
        keys: &'a K,
        reader: HidReader<'d, T, BUFFER_SIZE>,
        writer: HidWriter<'d, T, BUFFER_SIZE>,
    ) -> Self {
        Self {
            keys,
            reader: ContinuousReader::new(reader),
            writer: ContinuousWriter::new(writer),
        }
    }

    pub async fn com_loop(&mut self) -> ! {
        self.reader.reader.ready().await;
        loop {
            let hid_request = self.reader.pop().await.into();
            self.keys
                .handle_request(hid_request, &mut self.reader, &mut self.writer)
                .await;
            self.reader.flush();
        }
    }
}
