use quench_runtime::RootId;
use rustc_hash::{FxHashMap, FxHashSet};

pub(crate) struct State {
    next_server: u64,
    next_agent: u64,
    next_request: u64,
    next_response: u64,
    pub(crate) status_reasons: FxHashMap<u16, Vec<u8>>,
    pub(crate) response_factory: Option<RootId>,
    pub(crate) incoming_factory: Option<RootId>,
    pub(crate) response_tracking: Option<ResponseTracking>,
    pub(crate) servers: FxHashMap<u64, Server>,
    pub(crate) connections: FxHashMap<u64, ServerConnection>,
    pub(crate) responses: FxHashMap<u64, Response>,
    /// The collection epoch `responses` was last pruned at (`poll::prune_collected_responses`):
    /// an ended response's owner can only have died when a collection ran since.
    pub(crate) responses_pruned_epoch: u64,
    /// Responses whose write returned false, keyed by socket, until their
    /// queued bytes fall below the high-water mark and `drain` is due.
    pub(crate) drain_waiters: FxHashMap<u64, RootId>,
    pub(crate) requests: FxHashMap<u64, OutgoingRequest>,
    pub(crate) clients: FxHashMap<u64, Client>,
    pub(crate) agents: FxHashMap<u64, FxHashSet<u64>>,
}

impl State {
    /// Remove and return the response records of `socket`, which is closing.
    pub(crate) fn take_socket_responses(&mut self, socket: u64) -> Vec<Response> {
        let ids = self
            .responses
            .iter()
            .filter_map(|(id, response)| (response.socket == socket).then_some(*id))
            .collect::<Vec<_>>();
        ids.into_iter().filter_map(|id| self.responses.remove(&id)).collect()
    }
}

/// The objects every response is registered with as it is made (`response_initialize`): a
/// WeakMap from response to exchange id, which `_send` reads, with the intrinsic method that
/// records a response. A finished response's host entry is retired once its object is
/// collected (`poll::prune_collected_responses`).
#[derive(Clone, Copy)]
pub(crate) struct ResponseTracking {
    pub(crate) responses: RootId,
    pub(crate) map_set: RootId,
}

impl ResponseTracking {
    pub(crate) fn roots(self) -> [RootId; 2] {
        [self.responses, self.map_set]
    }
}

impl crate::host::startup_snapshot::HostSnap for ResponseTracking {
    fn write(&self, writer: &mut quench_runtime::SnapshotWriter<'_>) -> Result<(), String> {
        self.roots().iter().try_for_each(|root| root.write(writer))
    }
    fn read(reader: &mut quench_runtime::SnapshotReader<'_, '_>) -> Result<Self, String> {
        Ok(Self {
            responses: RootId::read(reader)?,
            map_set: RootId::read(reader)?,
        })
    }
}

pub(crate) struct Server {
    pub(crate) root: RootId,
    pub(crate) listener: Option<u64>,
    pub(crate) listening_pending: bool,
    pub(crate) closing: bool,
    pub(crate) connections: FxHashSet<u64>,
}

pub(crate) struct ServerConnection {
    pub(crate) server: u64,
    pub(crate) received: Vec<u8>,
    pub(crate) request_dispatched: bool,
}

pub(crate) struct Response {
    pub(crate) socket: u64,
    pub(crate) virtual_response: bool,
    pub(crate) async_id: u64,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Body,
    pub(crate) send_date: bool,
    pub(crate) http10: bool,
    pub(crate) use_chunked_encoding_by_default: bool,
    pub(crate) lifecycle: ResponseLifecycle,
    /// The guest response object, held weakly: an ended response keeps its record, which
    /// answers header reads and late writes, until that object is collected.
    pub(crate) owner: Option<quench_runtime::HostWeakRoot>,
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
            status_reasons: FxHashMap::default(),
            response_factory: None,
            incoming_factory: None,
            response_tracking: None,
            servers: FxHashMap::default(),
            connections: FxHashMap::default(),
            responses: FxHashMap::default(),
            responses_pruned_epoch: 0,
            drain_waiters: FxHashMap::default(),
            requests: FxHashMap::default(),
            clients: FxHashMap::default(),
            agents: FxHashMap::default(),
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
        self.agents.insert(id, FxHashSet::default());
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

impl State {
    pub(crate) fn write_snapshot(&self, writer: &mut quench_runtime::SnapshotWriter<'_>) -> Result<(), String> {
        use crate::host::startup_snapshot::{require_quiescent, HostSnap};
        let Self {
            next_server,
            next_agent,
            next_request,
            next_response,
            status_reasons,
            response_factory,
            incoming_factory,
            response_tracking,
            servers,
            connections,
            responses,
            // A collection epoch of this process's VM; a restored one counts its own.
            responses_pruned_epoch: _,
            drain_waiters,
            requests,
            clients,
            agents,
        } = self;
        require_quiescent(
            servers.is_empty()
                && connections.is_empty()
                && responses.is_empty()
                && drain_waiters.is_empty()
                && requests.is_empty()
                && clients.is_empty()
                && agents.values().all(|sockets| sockets.is_empty()),
            "an HTTP connection",
        )?;
        for counter in [next_server, next_agent, next_request, next_response] {
            counter.write(writer)?;
        }
        writer.len(status_reasons.len());
        for (code, reason) in status_reasons {
            writer.u32(u32::from(*code));
            writer.bytes(reason);
        }
        response_factory.write(writer)?;
        incoming_factory.write(writer)?;
        response_tracking.write(writer)?;
        // The agents the bootstrap made, which hold no socket yet.
        writer.len(agents.len());
        agents.keys().try_for_each(|agent| agent.write(writer))
    }
    pub(crate) fn read_snapshot(&mut self, reader: &mut quench_runtime::SnapshotReader<'_, '_>) -> Result<(), String> {
        use crate::host::startup_snapshot::HostSnap;
        for counter in [&mut self.next_server, &mut self.next_agent, &mut self.next_request, &mut self.next_response] {
            *counter = HostSnap::read(reader)?;
        }
        self.status_reasons.clear();
        for _ in 0..reader.len()? {
            let code = u16::try_from(reader.u32()?).map_err(|_| "invalid snapshot status code")?;
            self.status_reasons.insert(code, reader.bytes()?);
        }
        self.response_factory = HostSnap::read(reader)?;
        self.incoming_factory = HostSnap::read(reader)?;
        self.response_tracking = HostSnap::read(reader)?;
        self.agents.clear();
        for _ in 0..reader.len()? {
            self.agents.insert(HostSnap::read(reader)?, FxHashSet::default());
        }
        Ok(())
    }
}
