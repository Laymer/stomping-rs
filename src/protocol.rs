use std::{borrow::Cow, collections::BTreeMap};

use crate::errors::*;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum AckMode {
    Auto,
    ClientIndividual,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum Command {
    // Client Commands
    Connect,
    Send,
    Subscribe,
    Unsubscribe,
    Disconnect,
    Ack,

    // Server commands
    Connected,
    Message,
    Receipt,
    Error,
}

#[derive(Clone, Eq, PartialEq, Debug, Hash)]
pub struct Frame {
    pub command: Command,
    pub headers: Headers,
    pub body: Vec<u8>,
}
#[derive(Clone, Eq, PartialEq, Debug, Hash)]
pub struct StringyFrame {
    pub command: Command,
    pub headers: BTreeMap<(), ()>,
    pub body: (),
}

#[derive(Clone, Eq, PartialEq, Debug, Hash)]
pub enum FrameOrKeepAlive {
    Frame(Frame),
    KeepAlive,
}

impl AckMode {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            &AckMode::Auto => "auto",
            &AckMode::ClientIndividual => "client-individual",
        }
    }
}

pub type Headers = BTreeMap<Vec<u8>, Vec<u8>>;

impl Command {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Command::Connect => "CONNECT",
            Command::Send => "SEND",
            Command::Subscribe => "SUBSCRIBE",
            Command::Unsubscribe => "UNSUBSCRIBE",
            Command::Disconnect => "DISCONNECT",
            Command::Ack => "ACK",
            Command::Connected => "CONNECTED",
            Command::Message => "MESSAGE",
            Command::Receipt => "RECEIPT",
            Command::Error => "ERROR",
        }
    }
}

impl std::str::FromStr for Command {
    type Err = StompError;
    fn from_str(input: &str) -> Result<Self> {
        match input {
            "CONNECT" => Ok(Command::Connect),
            "SEND" => Ok(Command::Send),
            "SUBSCRIBE" => Ok(Command::Subscribe),
            "UNSUBSCRIBE" => Ok(Command::Unsubscribe),
            "DISCONNECT" => Ok(Command::Disconnect),
            "ACK" => Ok(Command::Ack),
            "CONNECTED" => Ok(Command::Connected),
            "MESSAGE" => Ok(Command::Message),
            "RECEIPT" => Ok(Command::Receipt),
            "ERROR" => Ok(Command::Error),
            _ => Err(StompError::ProtocolError),
        }
    }
}

impl Frame {
    pub(crate) fn stringify_headers(&self) -> BTreeMap<Cow<'_, str>, Cow<'_, str>> {
        self.headers
            .iter()
            .map(|(k, v)| (String::from_utf8_lossy(k), String::from_utf8_lossy(v)))
            .collect::<BTreeMap<_, _>>()
    }
}
