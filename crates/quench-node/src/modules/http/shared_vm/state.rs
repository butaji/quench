use rqj::RootId;
use std::collections::{HashMap, HashSet};

pub(crate) struct State {
    next_server: u64,
    next_agent: u64,
    next_response: u64,
    pub(crate) response_factory: Option<RootId>,
    pub(crate) servers: HashMap<u64, Server>,
    pub(crate) connections: HashMap<u64, ServerConnection>,
    pub(crate) responses: HashMap<u64, Response>,
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
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) ended: bool,
}

pub(crate) struct Client {
    pub(crate) callback: RootId,
    pub(crate) request: Vec<u8>,
    pub(crate) received: Vec<u8>,
}

impl State {
    pub(crate) fn new() -> Self {
        Self {
            next_server: 1,
            next_agent: 1,
            next_response: 1,
            response_factory: None,
            servers: HashMap::new(),
            connections: HashMap::new(),
            responses: HashMap::new(),
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
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}
