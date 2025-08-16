pub trait SlaveState: Eq + Ord + Clone + Copy {
    const DEFAULT: Self;
    fn update_state(&mut self, index: usize, pressed: bool);
    fn into_buffer(self, buf: &mut [u8]);
}

impl SlaveState for u32 {
    const DEFAULT: Self = 0;
    fn update_state(&mut self, index: usize, pressed: bool) {
        if pressed {
            *self |= 1 << index;
        } else {
            *self &= !(1 << index);
        }
    }

    fn into_buffer(self, buf: &mut [u8]) {
        buf[0..4].copy_from_slice(&self.to_le_bytes());
    }
}
#[allow(async_fn_in_trait)]
pub trait MasterRequest {
    type SlaveRespone: SlaveRespone;
}

#[allow(async_fn_in_trait)]
pub trait SlaveRespone {
    type MasterRequest: MasterRequest;
}
#[allow(async_fn_in_trait)]
pub trait Master {
    type Request: MasterRequest;
    type Response: SlaveRespone;
    type SlaveState;
    async fn send_request(&self, request: Self::Request);
    async fn get_response(&self) -> Self::Response;
    async fn get_slave_state(&self) -> Self::SlaveState;
    fn try_get_slave_state(&self) -> Option<Self::SlaveState>;
}

#[allow(async_fn_in_trait)]
pub trait Slave {
    type Request: MasterRequest;
    type Response: SlaveRespone;
    type SlaveState;

    async fn send_response(&self, message: Self::Response);
    async fn send_slave_state(&self, state: Self::SlaveState);
    async fn get_request(&self) -> Self::Request;
}
