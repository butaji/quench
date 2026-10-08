use quench_runtime::RootId;
use std::collections::{HashMap, HashSet};

pub(crate) struct State {
    next_server: u64,
    next_agent: u64,
    next_request: u64,
    next_response: u64,
    pub(crate) response_factory: Option<RootId>,
    pub(crate) incoming_factory: Option<RootId>,
    pub(crate) servers: HashMap<u64, Server>,
    pub(crate) connections: HashMap<u64, ServerConnection>,
    pub(crate) responses: HashMap<u64, Response>,
    pub(crate) requests: HashMap<u64, OutgoingRequest>,
    pub(crate) clients: HashMap<u64, Client>,
    pub(crate) agents: HashMap<u64, HashSet<u64>>,
}

pub(crate) struct Server {
    pub(crate) root: RootId,
    pub(crate) listener: Option<u64>,
    pub(crate) listening_pending: bool,
    pub(crate) closing: bool,
    pub(crate) connections: HashSet<u64>,
}

pub(crate) struct ServerConnection {
    pub(crate) server: u64,
    pub(crate) received: Vec<u8>,
    pub(crate) request_dispatched: bool,
}

pub(crate) struct Response {
    pub(crate) socket: u64,
    pub(crate) async_id: u64,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Body,
    pub(crate) send_date: bool,
    pub(crate) lifecycle: ResponseLifecycle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResponseLifecycle {
    Open,
    HeadersSent,
    Ended,
    Destroyed,
}

impl ResponseLifecycle {
    pub(crate) fn send_headers(&mut self) -> bool {
        if *self == Self::Open {
            *self = Self::HeadersSent;
            true
        } else {
            false
        }
    }

    pub(crate) fn end(&mut self) -> bool {
        if self.is_terminal() {
            false
        } else {
            *self = Self::Ended;
            true
        }
    }

    pub(crate) fn destroy(&mut self) -> bool {
        if self.is_terminal() {
            false
        } else {
            *self = Self::Destroyed;
            true
        }
    }

    pub(crate) fn is_terminal(self) -> bool {
        matches!(self, Self::Ended | Self::Destroyed)
    }
}

pub(crate) struct OutgoingRequest {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Body,
    pub(crate) agent: Option<u64>,
}

#[derive(Default)]
pub(crate) struct Body {
    writes: Vec<Vec<u8>>,
    ending: Option<Vec<u8>>,
}

impl Body {
    pub(crate) fn push(&mut self, bytes: Vec<u8>, explicit_write: bool) {
        if explicit_write {
            self.writes.push(bytes);
        } else {
            self.ending = Some(bytes);
        }
    }

    pub(crate) fn has_writes(&self) -> bool {
        !self.writes.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.writes.iter().map(Vec::len).sum::<usize>() + self.ending.as_ref().map_or(0, Vec::len)
    }

    pub(crate) fn bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.len());
        for chunk in self.chunks() {
            bytes.extend_from_slice(chunk);
        }
        bytes
    }

    pub(crate) fn chunks(&self) -> impl Iterator<Item = &[u8]> {
        self.writes
            .iter()
            .map(Vec::as_slice)
            .chain(self.ending.iter().map(Vec::as_slice))
    }
}

pub(crate) struct Client {
    pub(crate) request_id: u64,
    pub(crate) request_root: RootId,
    pub(crate) request: Vec<u8>,
    pub(crate) response_parser: super::protocol::ResponseParser,
    pub(crate) response_root: Option<RootId>,
}

impl State {
    pub(crate) fn new() -> Self {
        Self {
            next_server: 1,
            next_agent: 1,
            next_request: 1,
            next_response: 1,
            response_factory: None,
            incoming_factory: None,
            servers: HashMap::new(),
            connections: HashMap::new(),
            responses: HashMap::new(),
            requests: HashMap::new(),
            clients: HashMap::new(),
            agents: HashMap::new(),
        }
    }

    pub(crate) fn server_id(&mut self) -> Result<u64, String> {
        let id = self.next_server;
        self.next_server = id
            .checked_add(1)
            .ok_or_else(|| "HTTP server identifier space exhausted".to_owned())?;
        Ok(id)
    }

    pub(crate) fn agent_id(&mut self) -> Result<u64, String> {
        let id = self.next_agent;
        self.next_agent = id
            .checked_add(1)
            .ok_or_else(|| "HTTP agent identifier space exhausted".to_owned())?;
        self.agents.insert(id, HashSet::new());
        Ok(id)
    }

    pub(crate) fn response_id(&mut self) -> Result<u64, String> {
        let id = self.next_response;
        self.next_response = id
            .checked_add(1)
            .ok_or_else(|| "HTTP response identifier space exhausted".to_owned())?;
        Ok(id)
    }

    pub(crate) fn request_id(&mut self) -> Result<u64, String> {
        let id = self.next_request;
        self.next_request = id
            .checked_add(1)
            .ok_or_else(|| "HTTP request identifier space exhausted".to_owned())?;
        Ok(id)
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}
