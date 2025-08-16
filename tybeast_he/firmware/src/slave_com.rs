use core::{cell::RefCell, ops::DerefMut};

use embassy_futures::join::join;
use embassy_sync::{
    blocking_mutex::raw::ThreadModeRawMutex,
    channel::{Channel, Receiver, Sender},
};
use embassy_usb::{
    class::hid::{HidReader, HidReaderWriter, HidWriter},
    driver::Driver,
};
use key_lib::{
    descriptor::SlaveReport,
    slave_com::{Master, MasterRequest, Slave, SlaveRespone, SlaveState},
};

const CHANNEL_SIZE: usize = 5;

pub enum HidRequest {}

impl MasterRequest for HidRequest {
    type SlaveRespone = HidResponse;
}

pub enum HidResponse {
    None,
}

impl HidResponse {
    pub async fn get_response<'d, T: Driver<'d>>(buf: &[u8]) -> HidResponse {
        let slave_state = [buf[0], buf[1], buf[2], buf[3]];
        let slave_state = u32::from_le_bytes(slave_state);
        match buf[0] {
            0 => HidResponse::None,
            _ => HidResponse::None,
        }
    }

    pub async fn send_response<'d, T: Driver<'d>>(&self, buf: &mut [u8]) -> usize {
        match *self {
            HidResponse::None => {
                buf[0] = 0;
                return 1;
            }
        }
    }
}

impl SlaveRespone for HidResponse {
    type MasterRequest = HidRequest;
}

pub struct HidMasterTask {
    slave_chan: Channel<ThreadModeRawMutex, u32, CHANNEL_SIZE>,
    requests: Channel<ThreadModeRawMutex, HidRequest, CHANNEL_SIZE>,
    responses: Channel<ThreadModeRawMutex, HidResponse, CHANNEL_SIZE>,
}

#[allow(clippy::new_without_default)]
impl HidMasterTask {
    pub fn new() -> Self {
        Self {
            slave_chan: Channel::new(),
            requests: Channel::new(),
            responses: Channel::new(),
        }
    }

    pub fn chan(&self) -> HidMaster<'_> {
        HidMaster {
            slave_rec: self.slave_chan.receiver(),
            requests: self.requests.sender(),
            responses: self.responses.receiver(),
        }
    }

    pub async fn run<'d, T: Driver<'d>>(&self, hid: HidReaderWriter<'d, T, 32, 32>) {
        let (mut reader, mut writer) = hid.split();
        let read_loop = async {
            loop {
                let mut buf = [0u8; 32];
                reader.read(&mut buf).await;
                let slave_state = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
                self.slave_chan.send(slave_state).await;
            }
        };

        let write_loop = async {
            loop {
                let _ = self.requests.receive().await;
            }
        };
        join(read_loop, write_loop).await;
    }
}

pub struct HidMaster<'ch> {
    slave_rec: Receiver<'ch, ThreadModeRawMutex, u32, CHANNEL_SIZE>,
    requests: Sender<'ch, ThreadModeRawMutex, HidRequest, CHANNEL_SIZE>,
    responses: Receiver<'ch, ThreadModeRawMutex, HidResponse, CHANNEL_SIZE>,
}

impl<'ch> Master for HidMaster<'ch> {
    type Request = HidRequest;

    type Response = HidResponse;

    type SlaveState = u32;

    async fn send_request(&self, request: Self::Request) {
        self.requests.send(request).await;
    }

    async fn get_response(&self) -> Self::Response {
        self.responses.receive().await
    }

    async fn get_slave_state(&self) -> Self::SlaveState {
        self.slave_rec.receive().await
    }

    fn try_get_slave_state(&self) -> Option<Self::SlaveState> {
        self.slave_rec.try_receive().ok()
    }
}

pub struct HidSlaveTask {
    requests: Channel<ThreadModeRawMutex, HidRequest, CHANNEL_SIZE>,
    responses: Channel<ThreadModeRawMutex, HidResponse, CHANNEL_SIZE>,
    slave_state: Channel<ThreadModeRawMutex, u32, CHANNEL_SIZE>,
}

#[allow(clippy::new_without_default)]
impl HidSlaveTask {
    pub fn new() -> Self {
        Self {
            requests: Channel::new(),
            responses: Channel::new(),
            slave_state: Channel::new(),
        }
    }

    pub fn chan(&self) -> HidSlave<'_> {
        HidSlave {
            requests: self.requests.receiver(),
            responses: self.responses.sender(),
            slave_state: self.slave_state.sender(),
        }
    }

    pub async fn run<'d, T: Driver<'d>>(&self, hid: HidReaderWriter<'d, T, 32, 32>) {
        let (mut reader, mut writer) = hid.split();
        let read_loop = async {
            loop {
                let mut buf = [0u8; 32];
                reader.read(&mut buf).await.unwrap();
            }
        };

        let write_loop = async {
            let prev_slave_state = 0;
            loop {
                let mut slave_report = SlaveReport::default();
                let slave_state = self.slave_state.receive().await;
                slave_report.input[0..4].copy_from_slice(&slave_state.to_le_bytes());
                writer.write_serialize(&slave_report).await.unwrap();
            }
        };
        join(read_loop, write_loop).await;
    }
}

pub struct HidSlave<'ch> {
    requests: Receiver<'ch, ThreadModeRawMutex, HidRequest, CHANNEL_SIZE>,
    responses: Sender<'ch, ThreadModeRawMutex, HidResponse, CHANNEL_SIZE>,
    slave_state: Sender<'ch, ThreadModeRawMutex, u32, CHANNEL_SIZE>,
}

impl<'ch> Slave for HidSlave<'ch> {
    type Request = HidRequest;

    type Response = HidResponse;

    type SlaveState = u32;

    async fn send_response(&self, message: Self::Response) {
        self.responses.send(message).await;
    }

    async fn get_request(&self) -> Self::Request {
        self.requests.receive().await
    }

    async fn send_slave_state(&self, state: Self::SlaveState) {
        self.slave_state.send(state).await;
    }
}
