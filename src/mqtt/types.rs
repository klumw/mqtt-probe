//! Types shared between the UI thread and the MQTT worker thread.

use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;

use rust_i18n::t;
use tokio::sync::mpsc::UnboundedSender;

use crate::model::MessageRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Connected,
    Reconnecting,
    Disconnected,
    Error(String),
}

impl ConnectionState {
    pub fn label(&self) -> String {
        match self {
            Self::Connecting => t!("state.connecting").to_string(),
            Self::Connected => t!("state.connected").to_string(),
            Self::Reconnecting => t!("state.reconnecting").to_string(),
            Self::Disconnected => t!("state.disconnected").to_string(),
            Self::Error(_) => t!("state.error").to_string(),
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Connecting | Self::Connected | Self::Reconnecting
        )
    }
}

#[derive(Debug, Clone)]
pub enum WorkerEvent {
    State(ConnectionState),
    Message(MessageRecord),
    Log(String),
    Subscribed(String),
    SubscribeFailed(String),
}

#[derive(Debug, Clone)]
pub enum Command {
    Publish {
        topic: String,
        payload: Vec<u8>,
        qos: u8,
        retain: bool,
    },
    Disconnect,
    Shutdown,
}

pub struct ConnectionHandle {
    pub commands: UnboundedSender<Command>,
    pub events: Receiver<WorkerEvent>,
    #[allow(dead_code)]
    pub join: Option<JoinHandle<()>>,
}
