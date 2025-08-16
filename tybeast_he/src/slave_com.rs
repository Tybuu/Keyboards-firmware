use embassy_sync::{
    blocking_mutex::raw::ThreadModeRawMutex,
    channel::{Channel, Receiver, Sender},
};
use key_lib::slave_com::{Master, MasterRequest, Slave, SlaveRespone};

const CHANNEL_SIZE: usize = 5;

pub enum HidRequest {}

impl MasterRequest for HidRequest {
    type SlaveRespone = HidResponse;
}

pub enum HidResponse {}

impl SlaveRespone for HidResponse {
    type MasterRequest = HidRequest;
}

pub struct HidMasterTask {
    slave_chan: Channel<ThreadModeRawMutex, u32, CHANNEL_SIZE>,
    messages: Channel<ThreadModeRawMutex, HidRequest, CHANNEL_SIZE>,
    responses: Channel<ThreadModeRawMutex, HidResponse, CHANNEL_SIZE>,
}
#[allow(clippy::new_without_default)]
impl HidMasterTask {
    pub fn new() -> Self {
        Self {
            slave_chan: Channel::new(),
            messages: Channel::new(),
            responses: Channel::new(),
        }
    }

    pub fn chan(&self) -> HidMaster<'_> {
        HidMaster {
            slave_rec: self.slave_chan.receiver(),
            requests: self.messages.sender(),
            responses: self.responses.receiver(),
        }
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
}

pub struct HidSlaveTask {
    requests: Channel<ThreadModeRawMutex, HidRequest, CHANNEL_SIZE>,
    responses: Channel<ThreadModeRawMutex, HidResponse, CHANNEL_SIZE>,
}

#[allow(clippy::new_without_default)]
impl HidSlaveTask {
    pub fn new() -> Self {
        Self {
            requests: Channel::new(),
            responses: Channel::new(),
        }
    }

    pub fn chan(&self) -> HidSlave<'_> {
        HidSlave {
            requests: self.requests.receiver(),
            responses: self.responses.sender(),
        }
    }
}

pub struct HidSlave<'ch> {
    requests: Receiver<'ch, ThreadModeRawMutex, HidRequest, CHANNEL_SIZE>,
    responses: Sender<'ch, ThreadModeRawMutex, HidResponse, CHANNEL_SIZE>,
}

impl<'ch> Slave for HidSlave<'ch> {
    type Request = HidRequest;

    type Response = HidResponse;

    async fn send_response(&self, message: Self::Response) {
        self.responses.send(message).await;
    }

    async fn get_request(&self) -> Self::Request {
        self.requests.receive().await
    }
}
