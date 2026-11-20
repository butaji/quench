//! Main-thread c-ares channel with real sockets, timers and owned query completions.
//! No worker pool: c-ares owns DNS UDP/TCP I/O, driven by host readiness polling.
#[path = "cares_ffi.rs"]
mod ffi;
use ffi::*;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    ffi::{c_void, CStr, CString},
    marker::PhantomData,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    ptr::{self, NonNull},
    rc::Rc,
    sync::Mutex,
    time::Duration,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct QueryId(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryType {
    A,
    Aaaa,
    Cname,
    Ns,
    Ptr,
    Mx,
    Txt,
    Srv,
    Soa,
    Naptr,
    Caa,
    Tlsa,
    Any,
}
impl QueryType {
    fn native(self) -> i32 {
        match self {
            Self::A => ARES_REC_TYPE_A,
            Self::Aaaa => ARES_REC_TYPE_AAAA,
            Self::Cname => ARES_REC_TYPE_CNAME,
            Self::Ns => ARES_REC_TYPE_NS,
            Self::Ptr => ARES_REC_TYPE_PTR,
            Self::Mx => ARES_REC_TYPE_MX,
            Self::Txt => ARES_REC_TYPE_TXT,
            Self::Srv => ARES_REC_TYPE_SRV,
            Self::Soa => ARES_REC_TYPE_SOA,
            Self::Naptr => ARES_REC_TYPE_NAPTR,
            Self::Caa => ARES_REC_TYPE_CAA,
            Self::Tlsa => ARES_REC_TYPE_TLSA,
            Self::Any => ARES_REC_TYPE_ANY,
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum RData {
    A(Ipv4Addr),
    Aaaa(Ipv6Addr),
    Cname(Vec<u8>),
    Ns(Vec<u8>),
    Ptr(Vec<u8>),
    Mx {
        priority: u16,
        exchange: Vec<u8>,
    },
    Txt(Vec<Vec<u8>>),
    Srv {
        priority: u16,
        weight: u16,
        port: u16,
        name: Vec<u8>,
    },
    Soa {
        nsname: Vec<u8>,
        hostmaster: Vec<u8>,
        serial: u32,
        refresh: u32,
        retry: u32,
        expire: u32,
        minttl: u32,
    },
    Naptr {
        order: u16,
        preference: u16,
        flags: Vec<u8>,
        service: Vec<u8>,
        regexp: Vec<u8>,
        replacement: Vec<u8>,
    },
    Caa {
        critical: u8,
        tag: Vec<u8>,
        value: Vec<u8>,
    },
    Tlsa {
        usage: u8,
        selector: u8,
        matching_type: u8,
        data: Vec<u8>,
    },
}
#[derive(Debug, PartialEq, Eq)]
pub struct Record {
    pub ttl: u32,
    pub data: RData,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Failure {
    Cares { code: i32, timeouts: usize },
    MalformedRecord,
}
#[derive(Debug)]
pub struct Completion {
    pub id: QueryId,
    pub query_type: QueryType,
    pub result: Result<Vec<Record>, Failure>,
}
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub timeout_ms: i32,
    pub tries: i32,
    pub max_timeout_ms: i32,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            timeout_ms: -1,
            tries: 4,
            max_timeout_ms: 0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    pub address: IpAddr,
    pub udp_port: u16,
    pub tcp_port: u16,
}
#[derive(Debug)]
pub enum Error {
    Cares(i32),
    InteriorNul,
    IdentifierExhausted,
    PendingQueries,
    Closed,
    Io(std::io::Error),
}
static LIBRARY: Mutex<()> = Mutex::new(());
pub fn status_code(code: i32) -> &'static str {
    STATUS_CODES
        .iter()
        .find(|row| row.0 == code)
        .map_or("UNKNOWN_ARES_ERROR", |row| row.1)
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cares(code) => write!(
                f,
                "{}: {}",
                status_code(*code),
                Channel::error_string(*code)
            ),
            Self::Io(error) => error.fmt(f),
            Self::InteriorNul => f.write_str("DNS name contains a null byte"),
            Self::IdentifierExhausted => f.write_str("DNS query identifier space exhausted"),
            Self::PendingQueries => f.write_str("There are pending queries."),
            Self::Closed => f.write_str("DNS channel is closed"),
        }
    }
}
#[derive(Default)]
struct IoState {
    sockets: BTreeMap<i32, u32>,
    pending: BTreeSet<QueryId>,
    completions: VecDeque<Completion>,
}
#[derive(Default)]
struct Callbacks {
    state: RefCell<IoState>,
}
struct QueryToken {
    owner: NonNull<Callbacks>,
    id: QueryId,
    query_type: QueryType,
}
pub struct Channel {
    native: Option<NonNull<ffi::Channel>>,
    callbacks: Box<Callbacks>,
    next_query_id: u64,
    _main_thread: PhantomData<Rc<()>>,
}
unsafe extern "C" fn socket_state(data: *mut c_void, fd: i32, readable: i32, writable: i32) {
    let callbacks = unsafe { &*(data as *const Callbacks) };
    let mut state = callbacks.state.borrow_mut();
    let events = if readable != 0 {
        ARES_FD_EVENT_READ as u32
    } else {
        0
    } | if writable != 0 {
        ARES_FD_EVENT_WRITE as u32
    } else {
        0
    };
    if events == 0 {
        state.sockets.remove(&fd);
    } else {
        state.sockets.insert(fd, events);
    }
}
unsafe extern "C" fn query_complete(
    data: *mut c_void,
    status: i32,
    timeouts: usize,
    record: *const DnsRecord,
) {
    let token = unsafe { Box::from_raw(data as *mut QueryToken) };
    let result = if status == ARES_SUCCESS {
        unsafe {
            decode(record).and_then(|mut records| {
                decode_sdk_addresses(record, token.query_type, &mut records)?;
                Ok(records)
            })
        }
    } else {
        Err(Failure::Cares {
            code: status,
            timeouts,
        })
    };
    complete_query(*token, result);
}
fn complete_query(token: QueryToken, result: Result<Vec<Record>, Failure>) {
    let callbacks = unsafe { token.owner.as_ref() };
    let mut state = callbacks.state.borrow_mut();
    state.pending.remove(&token.id);
    state.completions.push_back(Completion {
        id: token.id,
        query_type: token.query_type,
        result,
    });
}
unsafe extern "C" fn reverse_complete(
    data: *mut c_void,
    status: i32,
    timeouts: i32,
    host: *mut libc::hostent,
) {
    let token = unsafe { Box::from_raw(data.cast::<QueryToken>()) };
    let result = if status != ARES_SUCCESS {
        Err(Failure::Cares {
            code: status,
            timeouts: timeouts.max(0) as usize,
        })
    } else if host.is_null() {
        Err(Failure::MalformedRecord)
    } else {
        let mut aliases = unsafe { (*host).h_aliases };
        let mut records = Vec::new();
        if !aliases.is_null() {
            while !unsafe { *aliases }.is_null() {
                records.push(Record {
                    ttl: 0,
                    data: RData::Ptr(unsafe { CStr::from_ptr(*aliases) }.to_bytes().to_vec()),
                });
                aliases = unsafe { aliases.add(1) };
            }
        }
        Ok(records)
    };
    complete_query(*token, result);
}
unsafe extern "C" {
    fn ares_gethostbyaddr(
        channel: *mut ffi::Channel,
        address: *const c_void,
        length: i32,
        family: i32,
        callback: unsafe extern "C" fn(*mut c_void, i32, i32, *mut libc::hostent),
        data: *mut c_void,
    );
}

impl Channel {
    pub fn new(config: Config) -> Result<Self, Error> {
        let status = {
            let _guard = LIBRARY.lock().expect("c-ares library lifecycle lock");
            unsafe { ares_library_init(ARES_LIB_INIT_ALL) }
        };
        if status != ARES_SUCCESS {
            return Err(Error::Cares(status));
        }
        let mut callbacks = Box::<Callbacks>::default();
        let mut options: Options = unsafe { std::mem::zeroed() };
        options.flags = ARES_FLAG_NOCHECKRESP;
        options.timeout = config.timeout_ms;
        options.tries = config.tries;
        options.maxtimeout = config.max_timeout_ms;
        options.sock_state_cb = Some(socket_state);
        options.sock_state_cb_data = (&mut *callbacks as *mut Callbacks).cast();
        options.qcache_max_ttl = 0;
        let mut mask = ARES_OPT_FLAGS
            | ARES_OPT_TIMEOUTMS
            | ARES_OPT_TRIES
            | ARES_OPT_SOCK_STATE_CB
            | ARES_OPT_QUERY_CACHE;
        if config.max_timeout_ms > 0 {
            mask |= ARES_OPT_MAXTIMEOUTMS;
        }
        let mut native = ptr::null_mut();
        let status = unsafe { ares_init_options(&mut native, &options, mask) };
        if status != ARES_SUCCESS {
            {
                let _guard = LIBRARY.lock().expect("c-ares library lifecycle lock");
                unsafe { ares_library_cleanup() }
            };
            return Err(Error::Cares(status));
        }
        Ok(Self {
            native: NonNull::new(native),
            callbacks,
            next_query_id: 0,
            _main_thread: PhantomData,
        })
    }
    fn native(&self) -> Result<*mut ffi::Channel, Error> {
        self.native.map(NonNull::as_ptr).ok_or(Error::Closed)
    }
    pub const fn expected_version() -> &'static str {
        CARES_VERSION
    }
    pub fn version() -> String {
        unsafe { CStr::from_ptr(ares_version(ptr::null_mut())) }
            .to_string_lossy()
            .into_owned()
    }
    pub fn error_string(code: i32) -> String {
        unsafe { CStr::from_ptr(ares_strerror(code)) }
            .to_string_lossy()
            .into_owned()
    }
    pub fn set_servers(&mut self, servers: &[Server]) -> Result<(), Error> {
        let native = self.native()?;
        if !self.callbacks.state.borrow().pending.is_empty() {
            return Err(Error::PendingQueries);
        }
        let mut nodes: Vec<ServerNode> = servers
            .iter()
            .map(|server| ServerNode {
                next: ptr::null_mut(),
                family: match server.address {
                    IpAddr::V4(_) => libc::AF_INET,
                    IpAddr::V6(_) => libc::AF_INET6,
                },
                address: match server.address {
                    IpAddr::V4(ip) => Address {
                        v4: libc::in_addr {
                            s_addr: u32::from_ne_bytes(ip.octets()),
                        },
                    },
                    IpAddr::V6(ip) => Address { v6: ip.octets() },
                },
                udp_port: i32::from(server.udp_port),
                tcp_port: i32::from(server.tcp_port),
            })
            .collect();
        for index in 0..nodes.len().saturating_sub(1) {
            nodes[index].next = unsafe { nodes.as_mut_ptr().add(index + 1) };
        }
        let status = unsafe {
            ares_set_servers_ports(
                native,
                if nodes.is_empty() {
                    ptr::null()
                } else {
                    nodes.as_ptr()
                },
            )
        };
        if status == ARES_SUCCESS {
            Ok(())
        } else {
            Err(Error::Cares(status))
        }
    }
    pub fn servers(&self) -> Result<Vec<Server>, Error> {
        let mut first = ptr::null_mut();
        let status = unsafe { ares_get_servers_ports(self.native()?, &mut first) };
        if status != ARES_SUCCESS {
            return Err(Error::Cares(status));
        }
        struct List(*mut ServerNode);
        impl Drop for List {
            fn drop(&mut self) {
                unsafe { ares_free_data(self.0.cast()) }
            }
        }
        let list = List(first);
        let mut cursor = list.0;
        let mut servers = Vec::new();
        while !cursor.is_null() {
            let node = unsafe { &*cursor };
            let address = match node.family {
                libc::AF_INET => IpAddr::V4(Ipv4Addr::from(
                    unsafe { node.address.v4.s_addr }.to_ne_bytes(),
                )),
                libc::AF_INET6 => IpAddr::V6(Ipv6Addr::from(unsafe { node.address.v6 })),
                _ => return Err(Error::Cares(ffi::ARES_EBADRESP)),
            };
            servers.push(Server {
                address,
                udp_port: node.udp_port as u16,
                tcp_port: node.tcp_port as u16,
            });
            cursor = node.next;
        }
        Ok(servers)
    }
    pub fn set_local_addresses(&mut self, v4: Ipv4Addr, v6: Ipv6Addr) -> Result<(), Error> {
        let native = self.native()?;
        unsafe {
            ares_set_local_ip4(native, u32::from_be_bytes(v4.octets()));
            ares_set_local_ip6(native, v6.octets().as_ptr());
        }
        Ok(())
    }
    pub fn query(&mut self, name: &str, query_type: QueryType) -> Result<QueryId, Error> {
        let native = self.native()?;
        let name = CString::new(name).map_err(|_| Error::InteriorNul)?;
        let (id, token) = self.query_token(query_type)?;
        // c-ares 1.34.8 guarantees callback on every submission path, including immediate errors.
        unsafe {
            ares_query_dnsrec(
                native,
                name.as_ptr(),
                ARES_CLASS_IN,
                query_type.native(),
                query_complete,
                Box::into_raw(token).cast(),
                ptr::null_mut(),
            );
        }
        Ok(id)
    }
    fn query_token(&mut self, query_type: QueryType) -> Result<(QueryId, Box<QueryToken>), Error> {
        let id = QueryId(self.next_query_id);
        self.next_query_id = self
            .next_query_id
            .checked_add(1)
            .ok_or(Error::IdentifierExhausted)?;
        self.callbacks.state.borrow_mut().pending.insert(id);
        let token = Box::new(QueryToken {
            owner: NonNull::from(&*self.callbacks),
            id,
            query_type,
        });
        Ok((id, token))
    }
    pub fn reverse(&mut self, address: IpAddr) -> Result<QueryId, Error> {
        let native = self.native()?;
        let (id, token) = self.query_token(QueryType::Ptr)?;
        let token = Box::into_raw(token).cast();
        match address {
            IpAddr::V4(ip) => {
                let bytes = ip.octets();
                unsafe {
                    ares_gethostbyaddr(
                        native,
                        bytes.as_ptr().cast(),
                        bytes.len() as i32,
                        libc::AF_INET,
                        reverse_complete,
                        token,
                    )
                };
            }
            IpAddr::V6(ip) => {
                let bytes = ip.octets();
                unsafe {
                    ares_gethostbyaddr(
                        native,
                        bytes.as_ptr().cast(),
                        bytes.len() as i32,
                        libc::AF_INET6,
                        reverse_complete,
                        token,
                    )
                };
            }
        }
        Ok(id)
    }
    pub fn has_work(&self) -> bool {
        let state = self.callbacks.state.borrow();
        !state.pending.is_empty() || !state.completions.is_empty()
    }
    pub fn pending_count(&self) -> usize {
        self.callbacks.state.borrow().pending.len()
    }
    pub fn next_timeout(&self) -> Option<Duration> {
        let native = self.native.ok_or(Error::Closed).ok()?;
        if self.pending_count() == 0 {
            return None;
        }
        let mut timeout: libc::timeval = unsafe { std::mem::zeroed() };
        let ptr = unsafe { ares_timeout(native.as_ptr(), ptr::null_mut(), &mut timeout) };
        if ptr.is_null() {
            None
        } else {
            let tv = unsafe { &*ptr };
            Some(
                Duration::from_secs(tv.tv_sec.max(0) as u64)
                    + Duration::from_micros(tv.tv_usec.max(0) as u64),
            )
        }
    }
    pub fn poll_ready(&mut self) -> Result<(), Error> {
        let native = self.native()?;
        let mut descriptors: Vec<libc::pollfd> = self
            .callbacks
            .state
            .borrow()
            .sockets
            .iter()
            .map(|(&fd, &events)| libc::pollfd {
                fd,
                events: if events & ARES_FD_EVENT_READ as u32 != 0 {
                    libc::POLLIN
                } else {
                    0
                } | if events & ARES_FD_EVENT_WRITE as u32 != 0 {
                    libc::POLLOUT
                } else {
                    0
                },
                revents: 0,
            })
            .collect();
        let count = unsafe {
            libc::poll(
                descriptors.as_mut_ptr(),
                descriptors.len() as libc::nfds_t,
                0,
            )
        };
        if count < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(Error::Io(error));
            }
        }
        let events: Vec<FdEvents> = descriptors
            .iter()
            .filter_map(|fd| {
                let read = fd.revents
                    & (libc::POLLIN | libc::POLLERR | libc::POLLHUP | libc::POLLNVAL)
                    != 0;
                let write = fd.revents & (libc::POLLOUT | libc::POLLNVAL) != 0;
                let events = if read { ARES_FD_EVENT_READ as u32 } else { 0 }
                    | if write { ARES_FD_EVENT_WRITE as u32 } else { 0 };
                (events != 0).then_some(FdEvents { fd: fd.fd, events })
            })
            .collect();
        let status = unsafe {
            ares_process_fds(
                native,
                if events.is_empty() {
                    ptr::null()
                } else {
                    events.as_ptr()
                },
                events.len(),
                ARES_PROCESS_FLAG_NONE as u32,
            )
        };
        if status == ARES_SUCCESS {
            Ok(())
        } else {
            Err(Error::Cares(status))
        }
    }
    pub fn take_completion(&mut self) -> Option<Completion> {
        self.callbacks.state.borrow_mut().completions.pop_front()
    }
    pub fn cancel(&mut self) -> Result<(), Error> {
        unsafe { ares_cancel(self.native()?) };
        Ok(())
    }
    pub fn close(&mut self) {
        if let Some(native) = self.native.take() {
            unsafe {
                ares_destroy(native.as_ptr());
            }
            let _guard = LIBRARY.lock().expect("c-ares library lifecycle lock");
            unsafe {
                ares_library_cleanup();
            }
        }
    }
}
impl Drop for Channel {
    fn drop(&mut self) {
        self.close();
    }
}
unsafe fn string(rr: *const Rr, key: i32) -> Result<Vec<u8>, Failure> {
    let ptr = unsafe { ares_dns_rr_get_str(rr, key) };
    if ptr.is_null() {
        Err(Failure::MalformedRecord)
    } else {
        Ok(unsafe { CStr::from_ptr(ptr) }.to_bytes().to_vec())
    }
}
unsafe fn binary(rr: *const Rr, key: i32) -> Result<Vec<u8>, Failure> {
    let mut length = 0;
    let ptr = unsafe { ares_dns_rr_get_bin(rr, key, &mut length) };
    if ptr.is_null() {
        if length == 0 {
            Ok(Vec::new())
        } else {
            Err(Failure::MalformedRecord)
        }
    } else {
        Ok(unsafe { std::slice::from_raw_parts(ptr, length) }.to_vec())
    }
}
unsafe fn decode(record: *const DnsRecord) -> Result<Vec<Record>, Failure> {
    if record.is_null() {
        return Err(Failure::MalformedRecord);
    }
    let mut output = Vec::new();
    for index in 0..unsafe { ares_dns_record_rr_cnt(record, ARES_SECTION_ANSWER) } {
        let rr = unsafe { ares_dns_record_rr_get_const(record, ARES_SECTION_ANSWER, index) };
        if rr.is_null() {
            return Err(Failure::MalformedRecord);
        }
        let kind = unsafe { ares_dns_rr_get_type(rr) };
        let s = |key| unsafe { string(rr, key) };
        let u16v = |key| unsafe { ares_dns_rr_get_u16(rr, key) };
        let u32v = |key| unsafe { ares_dns_rr_get_u32(rr, key) };
        let u8v = |key| unsafe { ares_dns_rr_get_u8(rr, key) };
        let data = match kind {
            ARES_REC_TYPE_A => {
                let ptr = unsafe { ares_dns_rr_get_addr(rr, ARES_RR_A_ADDR) };
                if ptr.is_null() {
                    return Err(Failure::MalformedRecord);
                }
                RData::A(Ipv4Addr::from(unsafe { (*ptr).s_addr }.to_ne_bytes()))
            }
            ARES_REC_TYPE_AAAA => {
                let ptr = unsafe { ares_dns_rr_get_addr6(rr, ARES_RR_AAAA_ADDR) };
                if ptr.is_null() {
                    return Err(Failure::MalformedRecord);
                }
                RData::Aaaa(Ipv6Addr::from(unsafe { *ptr }))
            }
            ARES_REC_TYPE_CNAME => RData::Cname(s(ARES_RR_CNAME_CNAME)?),
            ARES_REC_TYPE_NS => RData::Ns(s(ARES_RR_NS_NSDNAME)?),
            ARES_REC_TYPE_PTR => RData::Ptr(s(ARES_RR_PTR_DNAME)?),
            ARES_REC_TYPE_MX => RData::Mx {
                priority: u16v(ARES_RR_MX_PREFERENCE),
                exchange: s(ARES_RR_MX_EXCHANGE)?,
            },
            ARES_REC_TYPE_TXT => {
                let mut parts = Vec::new();
                for index in 0..unsafe { ares_dns_rr_get_abin_cnt(rr, ARES_RR_TXT_DATA) } {
                    let mut length = 0;
                    let ptr =
                        unsafe { ares_dns_rr_get_abin(rr, ARES_RR_TXT_DATA, index, &mut length) };
                    if ptr.is_null() {
                        return Err(Failure::MalformedRecord);
                    }
                    parts.push(unsafe { std::slice::from_raw_parts(ptr, length) }.to_vec());
                }
                RData::Txt(parts)
            }
            ARES_REC_TYPE_SRV => RData::Srv {
                priority: u16v(ARES_RR_SRV_PRIORITY),
                weight: u16v(ARES_RR_SRV_WEIGHT),
                port: u16v(ARES_RR_SRV_PORT),
                name: s(ARES_RR_SRV_TARGET)?,
            },
            ARES_REC_TYPE_SOA => RData::Soa {
                nsname: s(ARES_RR_SOA_MNAME)?,
                hostmaster: s(ARES_RR_SOA_RNAME)?,
                serial: u32v(ARES_RR_SOA_SERIAL),
                refresh: u32v(ARES_RR_SOA_REFRESH),
                retry: u32v(ARES_RR_SOA_RETRY),
                expire: u32v(ARES_RR_SOA_EXPIRE),
                minttl: u32v(ARES_RR_SOA_MINIMUM),
            },
            ARES_REC_TYPE_NAPTR => RData::Naptr {
                order: u16v(ARES_RR_NAPTR_ORDER),
                preference: u16v(ARES_RR_NAPTR_PREFERENCE),
                flags: s(ARES_RR_NAPTR_FLAGS)?,
                service: s(ARES_RR_NAPTR_SERVICES)?,
                regexp: s(ARES_RR_NAPTR_REGEXP)?,
                replacement: s(ARES_RR_NAPTR_REPLACEMENT)?,
            },
            ARES_REC_TYPE_CAA => RData::Caa {
                critical: u8v(ARES_RR_CAA_CRITICAL),
                tag: s(ARES_RR_CAA_TAG)?,
                value: unsafe { binary(rr, ARES_RR_CAA_VALUE) }?,
            },
            ARES_REC_TYPE_TLSA => RData::Tlsa {
                usage: u8v(ARES_RR_TLSA_CERT_USAGE),
                selector: u8v(ARES_RR_TLSA_SELECTOR),
                matching_type: u8v(ARES_RR_TLSA_MATCH),
                data: unsafe { binary(rr, ARES_RR_TLSA_DATA) }?,
            },
            // The SDK RR parsers ignore other answer types, including c-ares RAW_RR.
            _ => continue,
        };
        output.push(Record {
            ttl: unsafe { ares_dns_rr_get_ttl(rr) },
            data,
        });
    }
    Ok(output)
}
#[cfg(test)]
pub fn abi_layout() -> (usize, usize, usize, usize, usize) {
    (
        std::mem::size_of::<Options>(),
        std::mem::align_of::<Options>(),
        std::mem::offset_of!(Options, sock_state_cb),
        std::mem::offset_of!(Options, qcache_max_ttl),
        std::mem::offset_of!(Options, server_failover_opts),
    )
}

// Node's SDK deliberately uses the native c-ares hostent parsers for A,
// AAAA, CNAME and the first ANY groups. Reuse that authority for alias
// selection, owner filtering and CNAME-chain TTLs, rather than approximate it.
#[repr(C)]
struct AddrTtl {
    address: libc::in_addr,
    ttl: i32,
}
#[repr(C)]
struct Addr6Ttl {
    address: libc::in6_addr,
    ttl: i32,
}
unsafe extern "C" {
    fn ares_dns_write(record: *const DnsRecord, output: *mut *mut u8, length: *mut usize) -> i32;
    fn ares_free_string(data: *mut c_void);
    fn ares_free_hostent(host: *mut libc::hostent);
    fn ares_parse_a_reply(
        data: *const u8,
        length: i32,
        host: *mut *mut libc::hostent,
        ttls: *mut AddrTtl,
        count: *mut i32,
    ) -> i32;
    fn ares_parse_aaaa_reply(
        data: *const u8,
        length: i32,
        host: *mut *mut libc::hostent,
        ttls: *mut Addr6Ttl,
        count: *mut i32,
    ) -> i32;
}
struct WireReply(*mut u8);
impl Drop for WireReply {
    fn drop(&mut self) {
        unsafe { ares_free_string(self.0.cast()) }
    }
}
struct ParsedHost(*mut libc::hostent);
impl Drop for ParsedHost {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { ares_free_hostent(self.0) }
        }
    }
}
fn parsed_status(status: i32) -> Result<(), Failure> {
    if status == ARES_SUCCESS {
        Ok(())
    } else {
        Err(Failure::Cares {
            code: status,
            timeouts: 0,
        })
    }
}
unsafe fn decode_sdk_addresses(
    record: *const DnsRecord,
    query: QueryType,
    records: &mut Vec<Record>,
) -> Result<(), Failure> {
    if !matches!(
        query,
        QueryType::A | QueryType::Aaaa | QueryType::Cname | QueryType::Any
    ) {
        return Ok(());
    }
    let mut wire = ptr::null_mut();
    let mut wire_len = 0;
    parsed_status(unsafe { ares_dns_write(record, &mut wire, &mut wire_len) })?;
    let wire = WireReply(wire);
    let wire_len = i32::try_from(wire_len).map_err(|_| Failure::MalformedRecord)?;
    let answer_count = unsafe { ares_dns_record_rr_cnt(record, ARES_SECTION_ANSWER) }.max(1);
    let capacity = i32::try_from(answer_count).map_err(|_| Failure::MalformedRecord)?;
    records.retain(|record| !matches!(record.data, RData::A(_) | RData::Aaaa(_) | RData::Cname(_)));
    if matches!(query, QueryType::A | QueryType::Cname | QueryType::Any) {
        let mut host = ptr::null_mut();
        let mut ttls: Vec<AddrTtl> = (0..answer_count)
            .map(|_| unsafe { std::mem::zeroed() })
            .collect();
        let mut count = capacity;
        let status = unsafe {
            ares_parse_a_reply(wire.0, wire_len, &mut host, ttls.as_mut_ptr(), &mut count)
        };
        let host = ParsedHost(host);
        if status != ARES_ENODATA || query != QueryType::Any {
            parsed_status(status)?;
        }
        if status == ARES_SUCCESS {
            if host.0.is_null() || count < 0 || count > capacity {
                return Err(Failure::MalformedRecord);
            }
            let aliases = unsafe { (*host.0).h_aliases };
            let use_cname = query == QueryType::Cname
                || (query == QueryType::Any
                    && !aliases.is_null()
                    && !unsafe { *aliases }.is_null());
            if use_cname {
                let name = unsafe { (*host.0).h_name };
                if name.is_null() {
                    return Err(Failure::MalformedRecord);
                }
                records.push(Record {
                    ttl: 0,
                    data: RData::Cname(unsafe { CStr::from_ptr(name) }.to_bytes().to_vec()),
                });
            } else {
                for ttl in ttls.into_iter().take(count as usize) {
                    records.push(Record {
                        ttl: ttl.ttl as u32,
                        data: RData::A(Ipv4Addr::from(ttl.address.s_addr.to_ne_bytes())),
                    });
                }
            }
        }
    }
    if matches!(query, QueryType::Aaaa | QueryType::Any) {
        let mut host = ptr::null_mut();
        let mut ttls: Vec<Addr6Ttl> = (0..answer_count)
            .map(|_| unsafe { std::mem::zeroed() })
            .collect();
        let mut count = capacity;
        let status = unsafe {
            ares_parse_aaaa_reply(wire.0, wire_len, &mut host, ttls.as_mut_ptr(), &mut count)
        };
        let host = ParsedHost(host);
        if status != ARES_ENODATA || query != QueryType::Any {
            parsed_status(status)?;
        }
        if status == ARES_SUCCESS {
            if host.0.is_null() || count < 0 || count > capacity {
                return Err(Failure::MalformedRecord);
            }
            for ttl in ttls.into_iter().take(count as usize) {
                records.push(Record {
                    ttl: ttl.ttl as u32,
                    data: RData::Aaaa(Ipv6Addr::from(ttl.address.s6_addr)),
                });
            }
        }
    }
    Ok(())
}
