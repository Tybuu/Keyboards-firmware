pub trait MasterRequest {
    type SlaveRespone: SlaveRespone;
}

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
}

#[allow(async_fn_in_trait)]
pub trait Slave {
    type Request: MasterRequest;
    type Response: SlaveRespone;

    async fn send_response(&self, message: Self::Response);

    async fn get_request(&self) -> Self::Request;
}
