use core::{array, cell::RefCell, ops::DerefMut};

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

pub enum HidRequest {
    ConfigIndicate(u8),
    HallEffectReading(u8),
}

impl HidRequest {
    pub fn send_request(&self, buf: &mut [u8]) -> usize {
        match *self {
            HidRequest::ConfigIndicate(val) => {
                buf[0] = self.index() as u8;
                buf[1] = val;
                2
            }
            HidRequest::HallEffectReading(i) => {
                buf[0] = self.index() as u8;
                buf[1] = i;
                2
            }
        }
    }

    pub fn index(&self) -> usize {
        match self {
            Self::ConfigIndicate(_) => 0,
            Self::HallEffectReading(_) => 1,
        }
    }

    pub fn get_request(buf: &[u8]) -> Option<HidRequest> {
        match buf[0] {
            0 => Some(Self::ConfigIndicate(buf[1])),
            1 => Some(Self::HallEffectReading(buf[1])),
            _ => None,
        }
    }
}

impl MasterRequest for HidRequest {
    type SlaveRespone = HidResponse;
}

pub enum HidResponse {
    HallEffectReading(u16),
}

impl HidResponse {
    pub fn get_response(buf: &[u8]) -> Option<HidResponse> {
        const HALL_INDEX: u8 = HidResponse::HallEffectReading(0).index() as u8;
        match buf[0] {
            0 => None,
            HALL_INDEX => {
                let reading = u16::from_le_bytes([buf[1], buf[2]]);
                Some(HidResponse::HallEffectReading(reading))
            }
            _ => None,
        }
    }

    pub const fn index(&self) -> usize {
        match self {
            HidResponse::HallEffectReading(_) => 1,
        }
    }

    pub async fn send_response(&self, buf: &mut [u8]) -> usize {
        match *self {
            HidResponse::HallEffectReading(val) => {
                buf[0] = self.index() as u8;
                buf[1..3].copy_from_slice(&val.to_le_bytes());
                3
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
    responses: [Channel<ThreadModeRawMutex, HidResponse, CHANNEL_SIZE>;
        core::mem::variant_count::<HidResponse>()],
}

#[allow(clippy::new_without_default)]
impl HidMasterTask {
    pub fn new() -> Self {
        Self {
            slave_chan: Channel::new(),
            requests: Channel::new(),
            responses: array::from_fn(|_| Channel::new()),
        }
    }

    pub fn chan(&self) -> HidMaster<'_> {
        HidMaster {
            slave_rec: self.slave_chan.receiver(),
            requests: self.requests.sender(),
            responses: &self.responses,
        }
    }

    pub async fn run<'d, T: Driver<'d>>(&self, hid: HidReaderWriter<'d, T, 32, 32>) {
        let (mut reader, mut writer) = hid.split();
        let read_loop = async {
            loop {
                let mut buf = [0u8; 32];
                reader.read(&mut buf).await.unwrap();
                let slave_state = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
                self.slave_chan.send(slave_state).await;
                if let Some(resp) = HidResponse::get_response(&buf[4..]) {
                    self.responses[resp.index()].send(resp).await;
                }
            }
        };

        let write_loop = async {
            loop {
                let mut rep = SlaveReport::default();
                let req = self.requests.receive().await;
                req.send_request(&mut rep.input);
                writer.write_serialize(&rep).await.unwrap();
            }
        };
        join(read_loop, write_loop).await;
    }
}

pub struct HidMaster<'ch> {
    slave_rec: Receiver<'ch, ThreadModeRawMutex, u32, CHANNEL_SIZE>,
    requests: Sender<'ch, ThreadModeRawMutex, HidRequest, CHANNEL_SIZE>,
    responses: &'ch [Channel<ThreadModeRawMutex, HidResponse, CHANNEL_SIZE>;
             core::mem::variant_count::<HidResponse>()],
}

impl<'ch> HidMaster<'ch> {
    pub async fn get_response_copy(&self, resp: &mut HidResponse) {
        *resp = self.responses[resp.index()].receive().await;
    }

    pub fn try_send_request(&self, request: HidRequest) {
        self.requests.try_send(request);
    }
}

impl<'ch> Master for HidMaster<'ch> {
    type Request = HidRequest;

    type Response = HidResponse;

    type SlaveState = u32;

    async fn send_request(&self, request: Self::Request) {
        self.requests.send(request).await;
    }

    async fn get_response(&self) -> Self::Response {
        self.responses[0].receive().await
    }

    async fn get_slave_state(&self) -> Self::SlaveState {
        self.slave_rec.receive().await
    }

    fn try_get_slave_state(&self) -> Option<Self::SlaveState> {
        self.slave_rec.try_receive().ok()
    }
}

pub struct HidSlaveTask {
    requests: [Channel<ThreadModeRawMutex, HidRequest, CHANNEL_SIZE>;
        core::mem::variant_count::<HidRequest>()],
    responses: Channel<ThreadModeRawMutex, HidResponse, CHANNEL_SIZE>,
    slave_state: Channel<ThreadModeRawMutex, u32, CHANNEL_SIZE>,
}

#[allow(clippy::new_without_default)]
impl HidSlaveTask {
    pub fn new() -> Self {
        Self {
            requests: array::from_fn(|_| Channel::new()),
            responses: Channel::new(),
            slave_state: Channel::new(),
        }
    }

    pub fn chan(&self) -> HidSlave<'_> {
        HidSlave {
            requests: &self.requests,
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
                if let Some(req) = HidRequest::get_request(&buf) {
                    self.requests[req.index()].send(req).await;
                }
            }
        };

        let write_loop = async {
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
    requests: &'ch [Channel<ThreadModeRawMutex, HidRequest, CHANNEL_SIZE>;
             core::mem::variant_count::<HidRequest>()],
    responses: Sender<'ch, ThreadModeRawMutex, HidResponse, CHANNEL_SIZE>,
    slave_state: Sender<'ch, ThreadModeRawMutex, u32, CHANNEL_SIZE>,
}

impl<'ch> HidSlave<'ch> {
    pub async fn get_request_ref(&self, req: &mut HidRequest) {
        *req = self.requests[req.index()].receive().await;
    }
}

impl<'ch> Slave for HidSlave<'ch> {
    type Request = HidRequest;

    type Response = HidResponse;

    type SlaveState = u32;

    async fn send_response(&self, message: Self::Response) {
        self.responses.send(message).await;
    }

    async fn get_request(&self) -> Self::Request {
        self.requests[0].receive().await
    }

    async fn send_slave_state(&self, state: Self::SlaveState) {
        self.slave_state.send(state).await;
    }
}
