#![allow(dead_code)]
use std::ffi::{c_char, c_int, c_void};
include!("cares_constants.rs");
#[repr(C)]
pub struct Channel {
    _opaque: [u8; 0],
}
#[repr(C)]
pub struct DnsRecord {
    _opaque: [u8; 0],
}
#[repr(C)]
pub struct Rr {
    _opaque: [u8; 0],
}
#[repr(C)]
pub struct Failover {
    pub retry_chance: u16,
    pub retry_delay: usize,
}
#[repr(C)]
pub struct Options {
    pub flags: c_int,
    pub timeout: c_int,
    pub tries: c_int,
    pub ndots: c_int,
    pub udp_port: u16,
    pub tcp_port: u16,
    pub socket_send_buffer_size: c_int,
    pub socket_receive_buffer_size: c_int,
    pub servers: *mut libc::in_addr,
    pub nservers: c_int,
    pub domains: *mut *mut c_char,
    pub ndomains: c_int,
    pub lookups: *mut c_char,
    pub sock_state_cb: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int)>,
    pub sock_state_cb_data: *mut c_void,
    pub sortlist: *mut c_void,
    pub nsort: c_int,
    pub ednspsz: c_int,
    pub resolvconf_path: *mut c_char,
    pub hosts_path: *mut c_char,
    pub udp_max_queries: c_int,
    pub maxtimeout: c_int,
    pub qcache_max_ttl: u32,
    pub evsys: c_int,
    pub server_failover_opts: Failover,
}
#[repr(C)]
pub struct FdEvents {
    pub fd: c_int,
    pub events: u32,
}
#[repr(C)]
pub union Address {
    pub v4: libc::in_addr,
    pub v6: [u8; 16],
}
#[repr(C)]
pub struct ServerNode {
    pub next: *mut ServerNode,
    pub family: c_int,
    pub address: Address,
    pub udp_port: c_int,
    pub tcp_port: c_int,
}
#[link(name = "cares", kind = "static")]
unsafe extern "C" {
    pub fn ares_library_init(flags: c_int) -> c_int;
    pub fn ares_library_cleanup();
    pub fn ares_version(version: *mut c_int) -> *const c_char;
    pub fn ares_init_options(
        channel: *mut *mut Channel,
        options: *const Options,
        mask: c_int,
    ) -> c_int;
    pub fn ares_destroy(channel: *mut Channel);
    pub fn ares_cancel(channel: *mut Channel);
    pub fn ares_set_local_ip4(channel: *mut Channel, ip: u32);
    pub fn ares_set_local_ip6(channel: *mut Channel, ip: *const u8);
    pub fn ares_set_servers_ports(channel: *mut Channel, servers: *const ServerNode) -> c_int;
    pub fn ares_get_servers_ports(channel: *const Channel, servers: *mut *mut ServerNode) -> c_int;
    pub fn ares_free_data(data: *mut c_void);
    pub fn ares_timeout(
        channel: *const Channel,
        maxtv: *mut libc::timeval,
        tv: *mut libc::timeval,
    ) -> *mut libc::timeval;
    pub fn ares_process_fds(
        channel: *mut Channel,
        events: *const FdEvents,
        n: usize,
        flags: u32,
    ) -> c_int;
    pub fn ares_query_dnsrec(
        channel: *mut Channel,
        name: *const c_char,
        class: c_int,
        kind: c_int,
        callback: unsafe extern "C" fn(*mut c_void, c_int, usize, *const DnsRecord),
        arg: *mut c_void,
        qid: *mut u16,
    ) -> c_int;
    pub fn ares_strerror(code: c_int) -> *const c_char;
    pub fn ares_dns_record_rr_cnt(record: *const DnsRecord, section: c_int) -> usize;
    pub fn ares_dns_record_rr_get_const(
        record: *const DnsRecord,
        section: c_int,
        index: usize,
    ) -> *const Rr;
    pub fn ares_dns_rr_get_type(rr: *const Rr) -> c_int;
    pub fn ares_dns_rr_get_ttl(rr: *const Rr) -> u32;
    pub fn ares_dns_rr_get_addr(rr: *const Rr, key: c_int) -> *const libc::in_addr;
    pub fn ares_dns_rr_get_addr6(rr: *const Rr, key: c_int) -> *const [u8; 16];
    pub fn ares_dns_rr_get_str(rr: *const Rr, key: c_int) -> *const c_char;
    pub fn ares_dns_rr_get_u8(rr: *const Rr, key: c_int) -> u8;
    pub fn ares_dns_rr_get_bin(rr: *const Rr, key: c_int, length: *mut usize) -> *const u8;
    pub fn ares_dns_rr_get_u16(rr: *const Rr, key: c_int) -> u16;
    pub fn ares_dns_rr_get_u32(rr: *const Rr, key: c_int) -> u32;
    pub fn ares_dns_rr_get_abin_cnt(rr: *const Rr, key: c_int) -> usize;
    pub fn ares_dns_rr_get_abin(
        rr: *const Rr,
        key: c_int,
        index: usize,
        length: *mut usize,
    ) -> *const u8;
}
