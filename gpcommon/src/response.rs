use crate::vpn::VpnStatus;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Response {
    request_id: Option<u64>,
    success: bool,
    message: String,
    data: ResponseData,
}

impl From<ResponseData> for Response {
    fn from(data: ResponseData) -> Self {
        Self {
            request_id: None,
            success: true,
            message: String::from("Success"),
            data,
        }
    }
}

impl From<String> for Response {
    fn from(message: String) -> Self {
        Self {
            request_id: None,
            success: false,
            message,
            data: ResponseData::Empty,
        }
    }
}

impl Response {
    pub fn success(&self) -> bool {
        self.success
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn set_request_id(&mut self, command_id: u64) {
        self.request_id = Some(command_id);
    }

    pub fn request_id(&self) -> Option<u64> {
        self.request_id
    }

    pub fn data(&self) -> ResponseData {
        self.data
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum ResponseData {
    Status(VpnStatus),
    Empty,
}

impl From<VpnStatus> for ResponseData {
    fn from(status: VpnStatus) -> Self {
        Self::Status(status)
    }
}

impl From<()> for ResponseData {
    fn from(_: ()) -> Self {
        Self::Empty
    }
}

#[derive(Debug)]
pub struct TryFromResponseDataError {
    message: String,
}

impl std::fmt::Display for TryFromResponseDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Invalid ResponseData: {}", self.message)
    }
}

impl From<&str> for TryFromResponseDataError {
    fn from(message: &str) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl TryFrom<ResponseData> for VpnStatus {
    type Error = TryFromResponseDataError;

    fn try_from(value: ResponseData) -> Result<Self, Self::Error> {
        match value {
            ResponseData::Status(status) => Ok(status),
            _ => Err("ResponseData is not a VpnStatus".into()),
        }
    }
}

impl TryFrom<ResponseData> for () {
    type Error = TryFromResponseDataError;

    fn try_from(value: ResponseData) -> Result<Self, Self::Error> {
        match value {
            ResponseData::Empty => Ok(()),
            _ => Err("ResponseData is not empty".into()),
        }
    }
}
