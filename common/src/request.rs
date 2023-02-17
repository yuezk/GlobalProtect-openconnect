use crate::cmd::{Command, Connect, Disconnect, Status};
use crate::Response;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{oneshot, RwLock};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Request {
    id: u64,
    payload: CommandPayload,
}

impl Request {
    fn new(id: u64, payload: CommandPayload) -> Self {
        Self { id, payload }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn command(&self) -> Box<dyn Command> {
        match &self.payload {
            CommandPayload::Status(status) => Box::new(status.clone()),
            CommandPayload::Connect(connect) => Box::new(connect.clone()),
            CommandPayload::Disconnect(disconnect) => Box::new(disconnect.clone()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum CommandPayload {
    Status(Status),
    Connect(Connect),
    Disconnect(Disconnect),
}

impl From<Status> for CommandPayload {
    fn from(status: Status) -> Self {
        Self::Status(status)
    }
}

impl From<Connect> for CommandPayload {
    fn from(connect: Connect) -> Self {
        Self::Connect(connect)
    }
}

impl From<Disconnect> for CommandPayload {
    fn from(disconnect: Disconnect) -> Self {
        Self::Disconnect(disconnect)
    }
}

#[derive(Debug)]
struct RequestHandle {
    id: u64,
    response_tx: oneshot::Sender<Response>,
}

#[derive(Debug, Default)]
struct IdGenerator {
    current_id: u64,
}

impl IdGenerator {
    fn next(&mut self) -> u64 {
        let current_id = self.current_id;
        self.current_id = self.current_id.wrapping_add(1);
        current_id
    }
}

#[derive(Debug, Default)]
pub(crate) struct RequestPool {
    id_generator: Arc<RwLock<IdGenerator>>,
    request_handles: Arc<RwLock<Vec<RequestHandle>>>,
}

impl RequestPool {
    pub async fn create_request(
        &self,
        payload: CommandPayload,
    ) -> (Request, oneshot::Receiver<Response>) {
        let id = self.id_generator.write().await.next();
        let (response_tx, response_rx) = oneshot::channel();
        let request_handle = RequestHandle { id, response_tx };

        self.request_handles.write().await.push(request_handle);
        (Request::new(id, payload), response_rx)
    }

    pub async fn complete_request(&self, id: u64, response: Response) {
        let mut request_handles = self.request_handles.write().await;
        let request_handle = request_handles
            .iter()
            .position(|handle| handle.id == id)
            .map(|index| request_handles.remove(index));

        if let Some(request_handle) = request_handle {
            let _ = request_handle.response_tx.send(response);
        }
    }
}
