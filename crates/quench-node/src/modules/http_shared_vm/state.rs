use quench_runtime::RootId;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

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
    /// Each agent's pools, one entry per origin it has sockets for (`Agent.sockets`
    /// and `Agent.freeSockets` of the guest agent, merged per name).
    pub(crate) origins: FxHashMap<OriginKey, AgentOrigin>,
    /// Requests waiting for their origin's agent to fall below `maxSockets`.
    pub(crate) pending: VecDeque<PendingRequest>,
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
    pub(crate) upgraded: bool,
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
    /// The request's destination and, when it has an agent, its pool.
    pub(crate) origin: OriginKey,
    /// Whether the agent keeps this origin's socket for the next request.
    pub(crate) keep_alive: bool,
    pub(crate) max_sockets: usize,
    pub(crate) max_free_sockets: usize,
    /// Free-socket lifetime from `agent.options.timeout`; `None` never expires.
    pub(crate) idle_timeout: Option<Duration>,
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
    /// The exchange's destination and, for a request with an agent, its pool.
    pub(crate) origin: OriginKey,
    /// The exchange may leave its socket free for the next request.
    pub(crate) keep_alive: bool,
    pub(crate) max_sockets: usize,
    pub(crate) max_free_sockets: usize,
    /// Free-socket lifetime from `agent.options.timeout`; `None` never expires.
    pub(crate) idle_timeout: Option<Duration>,
    /// This exchange runs on a socket taken from the free pool, which Node reports
    /// as `ClientRequest.reusedSocket`.
    pub(crate) reused: bool,
}

/// Where an exchange connects and, when it has an agent, the pool that owns the
/// socket: Node's `Agent.getName` origin without the localAddress/family/
/// socketPath suffixes this host does not model.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct OriginKey {
    pub(crate) agent: Option<u64>,
    pub(crate) host: String,
    pub(crate) port: u16,
}

impl OriginKey {
    pub(crate) fn new(agent: Option<u64>, host: &str, port: u16) -> Self {
        Self {
            agent,
            host: host.to_owned(),
            port,
        }
    }

    /// Whether an agent pools this origin's sockets.
    pub(crate) fn pooled(&self) -> bool {
        self.agent.is_some()
    }
}

/// One agent's connections to one origin: every live socket it owns, in an
/// exchange or free, and the free subset in the order it was freed.
#[derive(Default)]
pub(crate) struct AgentOrigin {
    pub(crate) sockets: FxHashSet<u64>,
    pub(crate) idle: Vec<IdleSocket>,
}

/// A free socket and when the agent destroys it: `agent.options.timeout`, where
/// `None` never expires, as Node's `timeout: 0` never arms its timer.
pub(crate) struct IdleSocket {
    pub(crate) socket: u64,
    pub(crate) expires_at: Option<Instant>,
}

/// A request whose origin is at `maxSockets`, waiting for a socket to free.
pub(crate) struct PendingRequest {
    pub(crate) client: Client,
}

impl State {
    /// Record `socket` as owned by `key`'s agent.
    pub(crate) fn own_socket(&mut self, key: &OriginKey, socket: u64) {
        self.origins
            .entry(key.clone())
            .or_default()
            .sockets
            .insert(socket);
    }

    /// Whether `key`'s agent still owns `socket`.
    pub(crate) fn owns_socket(&self, key: &OriginKey, socket: u64) -> bool {
        self.origins
            .get(key)
            .is_some_and(|origin| origin.sockets.contains(&socket))
    }

    /// Register `socket` as free for `key`. Node destroys a socket that would
    /// exceed `maxFreeSockets` rather than evicting an older free socket, so a
    /// refused socket stays the caller's to close.
    pub(crate) fn free_socket(
        &mut self,
        key: &OriginKey,
        socket: u64,
        expires_at: Option<Instant>,
        max_free: usize,
    ) -> bool {
        let origin = self.origins.entry(key.clone()).or_default();
        if origin.idle.len() >= max_free {
            return false;
        }
        origin.idle.push(IdleSocket { socket, expires_at });
        true
    }

    /// Take the newest free socket of `key`, which Node's default LIFO
    /// scheduling reuses first.
    pub(crate) fn take_free_socket(&mut self, key: &OriginKey) -> Option<IdleSocket> {
        self.origins.get_mut(key)?.idle.pop()
    }

    /// Whether `key` has a free socket to reuse.
    pub(crate) fn has_free_socket(&self, key: &OriginKey) -> bool {
        self.origins
            .get(key)
            .is_some_and(|origin| !origin.idle.is_empty())
    }

    /// Whether `key`'s agent may open another socket: Node compares the origin's
    /// free and in-use socket counts with `maxSockets`.
    pub(crate) fn has_capacity(&self, key: &OriginKey, max_sockets: usize) -> bool {
        self.origins
            .get(key)
            .is_none_or(|origin| origin.sockets.len() < max_sockets)
    }

    /// Forget `socket` in `key`'s pool: its slot and its free entry.
    pub(crate) fn forget_socket(&mut self, key: &OriginKey, socket: u64) {
        let Some(origin) = self.origins.get_mut(key) else {
            return;
        };
        origin.sockets.remove(&socket);
        origin.idle.retain(|idle| idle.socket != socket);
        if origin.sockets.is_empty() {
            self.origins.remove(key);
        }
    }

    /// Take the free entry of a known `socket`, returning the origin that owns it
    /// so the caller can close the socket and drop its slot.
    pub(crate) fn take_free_socket_by_id(&mut self, socket: u64) -> Option<OriginKey> {
        let key = self.origins.iter().find_map(|(key, origin)| {
            origin
                .idle
                .iter()
                .any(|idle| idle.socket == socket)
                .then(|| key.clone())
        })?;
        if let Some(origin) = self.origins.get_mut(&key) {
            origin.idle.retain(|idle| idle.socket != socket);
        }
        Some(key)
    }
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
            origins: FxHashMap::default(),
            pending: VecDeque::new(),
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
            origins,
            pending,
        } = self;
        require_quiescent(
            servers.is_empty()
                && connections.is_empty()
                && responses.is_empty()
                && drain_waiters.is_empty()
                && requests.is_empty()
                && clients.is_empty()
                && origins.is_empty()
                && pending.is_empty(),
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
        Ok(())
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
        self.origins.clear();
        self.pending.clear();
        Ok(())
    }
}
