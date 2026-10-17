//! Resolver outcomes with c-ares' status codes, and the per-query projection of a decoded
//! response into the values Node's `cares_wrap` hands to JavaScript (src/cares_wrap.cc
//! over c-ares 1.34's legacy reply parsers).

use super::wire::{RecordData, RecordType, Response, Soa, CLASS_IN};
use std::net::{Ipv4Addr, Ipv6Addr};

/// c-ares status codes (`ares.h`); the discriminant is the code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub(crate) enum Status {
    NoData = 1,
    FormErr,
    ServFail,
    NotFound,
    NotImp,
    Refused,
    BadQuery,
    BadName,
    BadFamily,
    BadResp,
    ConnRefused,
    Timeout,
    Eof,
    File,
    NoMem,
    Destruction,
    BadStr,
    BadFlags,
    NoName,
    BadHints,
    NotInitialized,
    LoadIphlpapi,
    AddrGetNetworkParams,
    Cancelled,
    Service,
    NoServer,
}

/// Per status, in code order: Node's error code (`ToErrorCodeString`) and `ares_strerror`.
const STATUSES: [(Status, Option<&str>, &str); 26] = [
    (Status::NoData, Some("ENODATA"), "DNS server returned answer with no data"),
    (Status::FormErr, Some("EFORMERR"), "DNS server claims query was misformatted"),
    (Status::ServFail, Some("ESERVFAIL"), "DNS server returned general failure"),
    (Status::NotFound, Some("ENOTFOUND"), "Domain name not found"),
    (Status::NotImp, Some("ENOTIMP"), "DNS server does not implement requested operation"),
    (Status::Refused, Some("EREFUSED"), "DNS server refused query"),
    (Status::BadQuery, Some("EBADQUERY"), "Misformatted DNS query"),
    (Status::BadName, Some("EBADNAME"), "Misformatted domain name"),
    (Status::BadFamily, Some("EBADFAMILY"), "Unsupported address family"),
    (Status::BadResp, Some("EBADRESP"), "Misformatted DNS reply"),
    (Status::ConnRefused, Some("ECONNREFUSED"), "Could not contact DNS servers"),
    (Status::Timeout, Some("ETIMEOUT"), "Timeout while contacting DNS servers"),
    (Status::Eof, Some("EOF"), "End of file"),
    (Status::File, Some("EFILE"), "Error reading file"),
    (Status::NoMem, Some("ENOMEM"), "Out of memory"),
    (Status::Destruction, Some("EDESTRUCTION"), "Channel is being destroyed"),
    (Status::BadStr, Some("EBADSTR"), "Misformatted string"),
    (Status::BadFlags, Some("EBADFLAGS"), "Illegal flags specified"),
    (Status::NoName, Some("ENONAME"), "Given hostname is not numeric"),
    (Status::BadHints, Some("EBADHINTS"), "Illegal hints flags specified"),
    (Status::NotInitialized, Some("ENOTINITIALIZED"), "c-ares library initialization not yet performed"),
    (Status::LoadIphlpapi, Some("ELOADIPHLPAPI"), "Error loading iphlpapi.dll"),
    (Status::AddrGetNetworkParams, Some("EADDRGETNETWORKPARAMS"), "Could not find GetNetworkParams function"),
    (Status::Cancelled, Some("ECANCELLED"), "DNS query cancelled"),
    (Status::Service, None, "Invalid service name or number"),
    (Status::NoServer, None, "No DNS servers were configured"),
];
const SUCCESS_MESSAGE: &str = "Successful completion";
const UNKNOWN_MESSAGE: &str = "unknown";
const UNKNOWN_CODE: &str = "UNKNOWN_ARES_ERROR";
/// Node's own `setServers` status while queries are pending (src/cares_wrap.h).
pub(crate) const SET_SERVERS_PENDING: i32 = -1000;
const SET_SERVERS_PENDING_MESSAGE: &str = "There are pending queries.";

impl Status {
    fn row(self) -> &'static (Status, Option<&'static str>, &'static str) {
        &STATUSES[self as usize - 1]
    }

    /// Node's error code string for a failed query.
    pub(crate) fn code(self) -> &'static str {
        self.row().1.unwrap_or(UNKNOWN_CODE)
    }

    /// The c-ares status a DNS response code reports (`ares_dns_query_reply_tostatus`).
    pub(crate) fn of_response(response: &Response) -> Option<Self> {
        use super::wire::ResponseCode;
        match response.code {
            ResponseCode::NoError if response.answers.is_empty() => Some(Self::NoData),
            ResponseCode::NoError | ResponseCode::Other(_) => None,
            ResponseCode::FormErr => Some(Self::FormErr),
            ResponseCode::ServFail => Some(Self::ServFail),
            ResponseCode::NxDomain => Some(Self::NotFound),
            ResponseCode::NotImp => Some(Self::NotImp),
            ResponseCode::Refused => Some(Self::Refused),
        }
    }
}

/// `cares_wrap.strerror(code)`.
pub(crate) fn strerror(code: i32) -> &'static str {
    match code {
        0 => SUCCESS_MESSAGE,
        SET_SERVERS_PENDING => SET_SERVERS_PENDING_MESSAGE,
        _ => STATUSES
            .iter()
            .find(|row| row.0 as i32 == code)
            .map_or(UNKNOWN_MESSAGE, |row| row.2),
    }
}

/// The resolver queries `ChannelWrap` exposes, in Node's prototype order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QueryKind {
    Reverse,
    A,
    Any,
    Aaaa,
    Caa,
    Cname,
    Mx,
    Naptr,
    Ns,
    Ptr,
    Srv,
    Soa,
    Tlsa,
    Txt,
}

/// `ChannelWrap.prototype` query methods and the kind each sends.
pub(crate) const QUERIES: [(&str, QueryKind); 14] = [
    ("getHostByAddr", QueryKind::Reverse),
    ("queryA", QueryKind::A),
    ("queryAny", QueryKind::Any),
    ("queryAaaa", QueryKind::Aaaa),
    ("queryCaa", QueryKind::Caa),
    ("queryCname", QueryKind::Cname),
    ("queryMx", QueryKind::Mx),
    ("queryNaptr", QueryKind::Naptr),
    ("queryNs", QueryKind::Ns),
    ("queryPtr", QueryKind::Ptr),
    ("querySrv", QueryKind::Srv),
    ("querySoa", QueryKind::Soa),
    ("queryTlsa", QueryKind::Tlsa),
    ("queryTxt", QueryKind::Txt),
];

impl QueryKind {
    /// The record type the query asks for; a reverse lookup asks for PTR records.
    pub(crate) fn record_type(self) -> RecordType {
        match self {
            Self::Reverse | Self::Ptr => RecordType::Ptr,
            Self::A => RecordType::A,
            Self::Any => RecordType::Any,
            Self::Aaaa => RecordType::Aaaa,
            Self::Caa => RecordType::Caa,
            Self::Cname => RecordType::Cname,
            Self::Mx => RecordType::Mx,
            Self::Naptr => RecordType::Naptr,
            Self::Ns => RecordType::Ns,
            Self::Srv => RecordType::Srv,
            Self::Soa => RecordType::Soa,
            Self::Tlsa => RecordType::Tlsa,
            Self::Txt => RecordType::Txt,
        }
    }
}

/// A JavaScript value in an answer, built on the VM thread.
#[derive(Clone, Debug)]
pub(crate) enum Field {
    Text(String),
    Number(f64),
    ArrayBuffer(Vec<u8>),
    Strings(Vec<String>),
    Undefined,
}

/// An object's properties in creation order.
pub(crate) type Object = Vec<(String, Field)>;

#[derive(Clone, Debug)]
pub(crate) enum Item {
    Text(String),
    Strings(Vec<String>),
    Object(Object),
}

#[derive(Clone, Debug)]
pub(crate) enum Answer {
    /// An array; `None` entries are holes, as c-ares' TLSA projection indexes by record.
    List(Vec<Option<Item>>),
    Object(Object),
}

/// A successful query: `oncomplete(0, answer[, ttls])`.
#[derive(Clone, Debug)]
pub(crate) struct Reply {
    pub(crate) answer: Answer,
    pub(crate) ttls: Option<Vec<u32>>,
}

/// c-ares hands JavaScript one byte per character (V8 one-byte strings).
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&byte| char::from(byte)).collect()
}

fn text(value: impl Into<String>) -> Field {
    Field::Text(value.into())
}

fn number(value: impl Into<f64>) -> Field {
    Field::Number(value.into())
}

fn property(name: &str, value: Field) -> (String, Field) {
    (name.to_owned(), value)
}

/// TXT and CAA projections also accept CHAOS-class records.
const CLASS_CHAOS: u16 = 3;

fn records_in(response: &Response, kind: RecordType, chaos: bool) -> impl Iterator<Item = &RecordData> {
    response.answers.iter().filter_map(move |record| {
        let class_ok = record.class == CLASS_IN || (chaos && record.class == CLASS_CHAOS);
        (class_ok && record.kind == Some(kind)).then_some(&record.data)
    })
}

/// The address family a hostent projection keeps.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    V4,
    V6,
}

/// `ares_parse_a_reply`/`ares_parse_aaaa_reply` as a hostent: CNAMEs rename the target and
/// cap address TTLs; the name is the first CNAME target, or the last name in the chain.
struct Host {
    name: String,
    aliased: bool,
    addresses: Vec<(String, u32)>,
}

fn host(response: &Response, family: Family) -> Result<Host, Status> {
    let mut name = response.question.clone().unwrap_or_default();
    let mut first_target = None;
    let mut cname_ttl = u32::MAX;
    let mut nodes = Vec::new();
    let mut any = false;
    for record in response.answers.iter().filter(|record| record.class == CLASS_IN) {
        match &record.data {
            RecordData::Name(target) if record.kind == Some(RecordType::Cname) => {
                name = target.clone();
                first_target.get_or_insert_with(|| target.clone());
                cname_ttl = cname_ttl.min(record.ttl);
                any = true;
            }
            RecordData::A(octets) => {
                any = true;
                if family == Family::V4 {
                    nodes.push((Ipv4Addr::from(*octets).to_string(), record.ttl));
                }
            }
            RecordData::Aaaa(octets) => {
                any = true;
                if family == Family::V6 {
                    nodes.push((super::system::ipv6_text(Ipv6Addr::from(*octets)), record.ttl));
                }
            }
            _ => {}
        }
    }
    let aliased = first_target.is_some();
    if !any || (nodes.is_empty() && !aliased) {
        return Err(Status::NoData);
    }
    let addresses = nodes.into_iter().map(|(address, ttl)| (address, ttl.min(cname_ttl))).collect();
    Ok(Host { name: first_target.unwrap_or(name), aliased, addresses })
}

fn names(response: &Response, kind: RecordType) -> Result<Vec<String>, Status> {
    let names: Vec<String> = records_in(response, kind, false)
        .filter_map(|data| match data {
            RecordData::Name(name) => Some(name.clone()),
            _ => None,
        })
        .collect();
    if names.is_empty() {
        Err(Status::NoData)
    } else {
        Ok(names)
    }
}

fn mx(response: &Response, items: &mut Vec<Option<Item>>) {
    for data in records_in(response, RecordType::Mx, false) {
        if let RecordData::Mx { priority, exchange } = data {
            items.push(Some(Item::Object(vec![
                property("exchange", text(exchange.clone())),
                property("priority", number(*priority)),
                property("type", text("MX")),
            ])));
        }
    }
}

fn txt(response: &Response, typed: bool, items: &mut Vec<Option<Item>>) {
    for data in records_in(response, RecordType::Txt, true) {
        let RecordData::Txt(strings) = data else { continue };
        if strings.is_empty() {
            continue;
        }
        let entries = strings.iter().map(|string| latin1(string)).collect();
        items.push(Some(if typed {
            Item::Object(vec![property("entries", Field::Strings(entries)), property("type", text("TXT"))])
        } else {
            Item::Strings(entries)
        }));
    }
}

fn srv(response: &Response, items: &mut Vec<Option<Item>>) {
    for data in records_in(response, RecordType::Srv, false) {
        if let RecordData::Srv { priority, weight, port, target } = data {
            items.push(Some(Item::Object(vec![
                property("name", text(target.clone())),
                property("port", number(*port)),
                property("priority", number(*priority)),
                property("weight", number(*weight)),
                property("type", text("SRV")),
            ])));
        }
    }
}

fn naptr(response: &Response, typed: bool, items: &mut Vec<Option<Item>>) {
    for data in records_in(response, RecordType::Naptr, false) {
        if let RecordData::Naptr { order, preference, flags, service, regexp, replacement } = data {
            items.push(Some(Item::Object(vec![
                property("flags", text(latin1(flags))),
                property("service", text(latin1(service))),
                property("regexp", text(latin1(regexp))),
                property("replacement", text(replacement.clone())),
                property("order", number(*order)),
                property("preference", number(*preference)),
                property("type", if typed { text("NAPTR") } else { Field::Undefined }),
            ])));
        }
    }
}

fn soa_object(soa: &Soa, typed: bool) -> Object {
    vec![
        property("nsname", text(soa.nsname.clone())),
        property("hostmaster", text(soa.hostmaster.clone())),
        property("serial", number(soa.serial)),
        property("refresh", number(soa.refresh)),
        property("retry", number(soa.retry)),
        property("expire", number(soa.expire)),
        property("minttl", number(soa.minttl)),
        property("type", if typed { text("SOA") } else { Field::Undefined }),
    ]
}

/// TLSA records keep their answer index: other records leave holes.
fn tlsa(response: &Response, items: &mut Vec<Option<Item>>) {
    let offset = items.len();
    for (index, record) in response.answers.iter().enumerate() {
        let RecordData::Tlsa { usage, selector, matching, data } = &record.data else { continue };
        if data.is_empty() {
            continue;
        }
        if items.len() <= offset + index {
            items.resize(offset + index + 1, None);
        }
        items[offset + index] = Some(Item::Object(vec![
            property("certUsage", number(*usage)),
            property("selector", number(*selector)),
            property("match", number(*matching)),
            property("data", Field::ArrayBuffer(data.clone())),
        ]));
    }
}

fn caa(response: &Response, items: &mut Vec<Option<Item>>) {
    for data in records_in(response, RecordType::Caa, true) {
        if let RecordData::Caa { flags, tag, value } = data {
            items.push(Some(Item::Object(vec![
                property("critical", number(*flags)),
                property("type", text("CAA")),
                (latin1(tag), text(latin1(value))),
            ])));
        }
    }
}

fn list(items: Vec<Option<Item>>) -> Reply {
    Reply { answer: Answer::List(items), ttls: None }
}

fn texts(values: impl IntoIterator<Item = String>) -> Vec<Option<Item>> {
    values.into_iter().map(|value| Some(Item::Text(value))).collect()
}

fn addresses(response: &Response, family: Family) -> Result<Reply, Status> {
    let host = host(response, family)?;
    let (addresses, ttls): (Vec<_>, Vec<_>) = host.addresses.into_iter().unzip();
    Ok(Reply { answer: Answer::List(texts(addresses)), ttls: Some(ttls) })
}

/// Project an answered query; `Err` is the status the query fails with.
pub(crate) fn project(kind: QueryKind, response: &Response) -> Result<Reply, Status> {
    let mut items = Vec::new();
    match kind {
        QueryKind::A => addresses(response, Family::V4),
        QueryKind::Aaaa => addresses(response, Family::V6),
        QueryKind::Cname => Ok(list(texts([host(response, Family::V4)?.name]))),
        QueryKind::Ns => Ok(list(texts(names(response, RecordType::Ns)?))),
        QueryKind::Ptr | QueryKind::Reverse => Ok(list(texts(names(response, RecordType::Ptr)?))),
        QueryKind::Mx => {
            mx(response, &mut items);
            Ok(list(items))
        }
        QueryKind::Txt => {
            txt(response, false, &mut items);
            Ok(list(items))
        }
        QueryKind::Srv => {
            srv(response, &mut items);
            Ok(list(items))
        }
        QueryKind::Naptr => {
            naptr(response, false, &mut items);
            Ok(list(items))
        }
        QueryKind::Soa => records_in(response, RecordType::Soa, false)
            .find_map(|data| match data {
                RecordData::Soa(soa) => Some(Reply { answer: Answer::Object(soa_object(soa, false)), ttls: None }),
                _ => None,
            })
            .ok_or(Status::BadResp),
        QueryKind::Tlsa => {
            tlsa(response, &mut items);
            Ok(list(items))
        }
        QueryKind::Caa => {
            caa(response, &mut items);
            Ok(list(items))
        }
        QueryKind::Any => any(response),
    }
}

/// `AnyTraits::Parse`: every projection in Node's order, each tolerating ENODATA.
fn any(response: &Response) -> Result<Reply, Status> {
    let tolerate = |result: Result<(), Status>| match result {
        Err(Status::NoData) | Ok(()) => Ok(()),
        Err(status) => Err(status),
    };
    let mut items: Vec<Option<Item>> = Vec::new();
    let typed = |value: String, kind: &str| {
        Some(Item::Object(vec![property("value", text(value)), property("type", text(kind))]))
    };
    let address = |(address, ttl): (String, u32), kind: &str| {
        Some(Item::Object(vec![property("address", text(address)), property("ttl", number(ttl)), property("type", text(kind))]))
    };
    tolerate(host(response, Family::V4).map(|host| {
        if host.aliased {
            items.push(typed(host.name, "CNAME"));
        } else {
            items.extend(host.addresses.into_iter().map(|entry| address(entry, "A")));
        }
    }))?;
    tolerate(host(response, Family::V6).map(|host| {
        items.extend(host.addresses.into_iter().map(|entry| address(entry, "AAAA")));
    }))?;
    mx(response, &mut items);
    tolerate(names(response, RecordType::Ns).map(|names| items.extend(names.into_iter().map(|name| typed(name, "NS")))))?;
    txt(response, true, &mut items);
    srv(response, &mut items);
    tolerate(names(response, RecordType::Ptr).map(|names| items.extend(names.into_iter().map(|name| typed(name, "PTR")))))?;
    naptr(response, true, &mut items);
    if let Some(RecordData::Soa(soa)) = response.answers.iter().find(|record| record.kind == Some(RecordType::Soa)).map(|record| &record.data) {
        items.push(Some(Item::Object(soa_object(soa, true))));
    }
    tlsa(response, &mut items);
    caa(response, &mut items);
    Ok(list(items))
}
