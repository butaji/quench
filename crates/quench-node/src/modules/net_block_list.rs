//! `net.BlockList` and `net.SocketAddress`. An address is one 128-bit value in which an
//! IPv4 address is its IPv4-mapped IPv6 form, so a rule of either family applies to both
//! spellings of the same host. Each object keeps its native state as a byte record under a
//! private slot: a SocketAddress one record, a BlockList its rules newest first.

use crate::host::NodeHost;
use quench_runtime::{DescriptorFields, NativeContext, RootId, RootedError};
use std::net::{Ipv4Addr, Ipv6Addr};

type Context<'a> = NativeContext<'a, NodeHost>;
type Outcome = Result<RootId, RootedError>;

const IPV4_MAPPED_PREFIX: u128 = 0xffff_0000_0000;
const IPV4_BITS: u8 = 32;
const IPV6_BITS: u8 = 128;
const ADDRESS_BYTES: usize = 16;
const PORT_BYTES: usize = 2;
const FLOWLABEL_BYTES: usize = 4;
const MAX_PORT: f64 = u16::MAX as f64;

/// A SocketAddress record: family, address, port, flow label.
const SOCKET_FAMILY: usize = 0;
const SOCKET_ADDRESS: usize = SOCKET_FAMILY + 1;
const SOCKET_PORT: usize = SOCKET_ADDRESS + ADDRESS_BYTES;
const SOCKET_FLOWLABEL: usize = SOCKET_PORT + PORT_BYTES;
const SOCKET_RECORD: usize = SOCKET_FLOWLABEL + FLOWLABEL_BYTES;

/// A BlockList rule record: kind, family, subnet prefix, first and last address.
const RULE_KIND: usize = 0;
const RULE_FAMILY: usize = RULE_KIND + 1;
const RULE_PREFIX: usize = RULE_FAMILY + 1;
const RULE_START: usize = RULE_PREFIX + 1;
const RULE_END: usize = RULE_START + ADDRESS_BYTES;
const RULE_RECORD: usize = RULE_END + ADDRESS_BYTES;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    V4,
    V6,
}

impl Family {
    const ALL: [Self; 2] = [Self::V4, Self::V6];

    fn tag(self) -> u8 {
        match self {
            Self::V4 => super::net_config::IPV4_FAMILY,
            Self::V6 => super::net_config::IPV6_FAMILY,
        }
    }
    fn from_tag(tag: u8) -> Self {
        if tag == super::net_config::IPV4_FAMILY { Self::V4 } else { Self::V6 }
    }
    /// The option spelling (`'ipv4'`), as `family` options and SocketAddress report it.
    fn option(self) -> &'static str {
        match self {
            Self::V4 => "ipv4",
            Self::V6 => "ipv6",
        }
    }
    /// The rule spelling (`'IPv4'`).
    fn label(self) -> &'static str {
        match self {
            Self::V4 => "IPv4",
            Self::V6 => "IPv6",
        }
    }
    fn bits(self) -> u8 {
        match self {
            Self::V4 => IPV4_BITS,
            Self::V6 => IPV6_BITS,
        }
    }
    fn default_address(self) -> &'static str {
        match self {
            Self::V4 => "127.0.0.1",
            Self::V6 => "::",
        }
    }
    fn parse(self, text: &str) -> Option<u128> {
        match self {
            Self::V4 => text.parse::<Ipv4Addr>().ok().map(|ip| IPV4_MAPPED_PREFIX | u128::from(u32::from(ip))),
            Self::V6 => {
                let address = text.split_once('%').map_or(text, |(address, _)| address);
                address.parse::<Ipv6Addr>().ok().map(u128::from)
            }
        }
    }
    fn text(self, value: u128) -> String {
        match self {
            Self::V4 => Ipv4Addr::from(value as u32).to_string(),
            Self::V6 => Ipv6Addr::from(value).to_string(),
        }
    }
    /// A prefix of this family as a prefix of the unified 128-bit value.
    fn unified_prefix(self, prefix: u8) -> u8 {
        IPV6_BITS - self.bits() + prefix
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RuleKind {
    Address,
    Range,
    Subnet,
}

impl RuleKind {
    const ALL: [Self; 3] = [Self::Address, Self::Range, Self::Subnet];

    fn label(self) -> &'static str {
        match self {
            Self::Address => "Address",
            Self::Range => "Range",
            Self::Subnet => "Subnet",
        }
    }
}

#[derive(Clone, Copy)]
struct SocketAddress {
    family: Family,
    address: u128,
    port: u16,
    flowlabel: u32,
}

impl SocketAddress {
    fn encode(self) -> [u8; SOCKET_RECORD] {
        let mut record = [0; SOCKET_RECORD];
        record[SOCKET_FAMILY] = self.family.tag();
        record[SOCKET_ADDRESS..SOCKET_PORT].copy_from_slice(&self.address.to_be_bytes());
        record[SOCKET_PORT..SOCKET_FLOWLABEL].copy_from_slice(&self.port.to_be_bytes());
        record[SOCKET_FLOWLABEL..].copy_from_slice(&self.flowlabel.to_be_bytes());
        record
    }
    fn decode(record: &[u8]) -> Self {
        Self {
            family: Family::from_tag(record[SOCKET_FAMILY]),
            address: u128::from_be_bytes(record[SOCKET_ADDRESS..SOCKET_PORT].try_into().expect("address field")),
            port: u16::from_be_bytes(record[SOCKET_PORT..SOCKET_FLOWLABEL].try_into().expect("port field")),
            flowlabel: u32::from_be_bytes(record[SOCKET_FLOWLABEL..SOCKET_RECORD].try_into().expect("flowlabel field")),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Rule {
    kind: RuleKind,
    family: Family,
    prefix: u8,
    start: u128,
    end: u128,
}

impl Rule {
    fn encode(self, into: &mut Vec<u8>) {
        into.push(RuleKind::ALL.iter().position(|kind| *kind == self.kind).expect("listed kind") as u8);
        into.push(self.family.tag());
        into.push(self.prefix);
        into.extend_from_slice(&self.start.to_be_bytes());
        into.extend_from_slice(&self.end.to_be_bytes());
    }
    fn decode(record: &[u8]) -> Self {
        Self {
            kind: RuleKind::ALL[usize::from(record[RULE_KIND])],
            family: Family::from_tag(record[RULE_FAMILY]),
            prefix: record[RULE_PREFIX],
            start: u128::from_be_bytes(record[RULE_START..RULE_END].try_into().expect("start field")),
            end: u128::from_be_bytes(record[RULE_END..RULE_RECORD].try_into().expect("end field")),
        }
    }
    fn applies(self, address: u128) -> bool {
        match self.kind {
            RuleKind::Address => address == self.start,
            RuleKind::Range => (self.start..=self.end).contains(&address),
            RuleKind::Subnet => {
                let shift = IPV6_BITS - self.family.unified_prefix(self.prefix);
                let mask = u128::MAX.checked_shl(u32::from(shift)).unwrap_or(0);
                address & mask == self.start & mask
            }
        }
    }
    fn text(self) -> String {
        let start = self.family.text(self.start);
        let kind = self.kind.label();
        let family = self.family.label();
        match self.kind {
            RuleKind::Address => format!("{kind}: {family} {start}"),
            RuleKind::Range => format!("{kind}: {family} {start}-{}", self.family.text(self.end)),
            RuleKind::Subnet => format!("{kind}: {family} {start}/{}", self.prefix),
        }
    }
}

/// The module's fixed members: the record slots and the two classes.
struct Intrinsics {
    socket_slot: RootId,
    rules_slot: RootId,
    socket_address: RootId,
}

fn intrinsics(c: &mut Context<'_>) -> Result<Intrinsics, RootedError> {
    let holder = crate::host::shared_vm::commonjs::cached_native_binding(c, "\0net:block-list", build)?;
    Ok(Intrinsics {
        socket_slot: get(c, holder, "socketSlot")?,
        rules_slot: get(c, holder, "rulesSlot")?,
        socket_address: get(c, holder, "SocketAddress")?,
    })
}

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Outcome {
    c.get_named_property_rooted(object, name)
}

fn define(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, false, true)?;
    Ok(())
}

fn registered_symbol(c: &mut Context<'_>, key: &str) -> Outcome {
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let registry = get(c, symbol, "for")?;
    let key = c.string_rooted(key);
    c.call_rooted(registry, symbol, &[key])
}

fn methods(c: &mut Context<'_>, target: RootId, members: &[(&str, &str)]) -> Result<(), RootedError> {
    for &(name, operation) in members {
        let function = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        define(c, target, name, function)?;
    }
    Ok(())
}

fn getters(c: &mut Context<'_>, target: RootId, members: &[(&str, &str)]) -> Result<(), RootedError> {
    for &(name, operation) in members {
        let getter = c.host_function_named(crate::host::shared_vm::operation(operation), &format!("get {name}"))?;
        let key = c.string_rooted(name);
        let fields = DescriptorFields {
            get: Some(getter),
            set: Some(c.undefined()),
            enumerable: Some(false),
            configurable: Some(true),
            ..DescriptorFields::default()
        };
        c.define_property_fields_rooted(target, key, fields)?;
    }
    Ok(())
}

fn build(c: &mut Context<'_>) -> Outcome {
    let holder = c.null_object_rooted()?;
    let socket_slot = c.private_symbol_rooted(Some("kHandle"));
    define(c, holder, "socketSlot", socket_slot)?;
    let rules_slot = c.private_symbol_rooted(Some("kHandle"));
    define(c, holder, "rulesSlot", rules_slot)?;
    let inspect = registered_symbol(c, "nodejs.util.inspect.custom")?;

    let socket_address = c.host_constructor_named(crate::host::shared_vm::operation("netSocketAddressConstruct"), "SocketAddress")?;
    let prototype = get(c, socket_address, "prototype")?;
    getters(c, prototype, &[
        ("address", "netSocketAddressAddress"),
        ("port", "netSocketAddressPort"),
        ("family", "netSocketAddressFamily"),
        ("flowlabel", "netSocketAddressFlowlabel"),
    ])?;
    methods(c, prototype, &[("toJSON", "netSocketAddressToJson")])?;
    let custom = c.host_function_named(crate::host::shared_vm::operation("netSocketAddressInspect"), "[nodejs.util.inspect.custom]")?;
    c.define_data_property_rooted(prototype, inspect, custom, true, false, true)?;
    methods(c, socket_address, &[
        ("isSocketAddress", "netIsSocketAddress"),
        ("parse", "netSocketAddressParse"),
    ])?;
    define(c, holder, "SocketAddress", socket_address)?;

    let block_list = c.host_constructor_named(crate::host::shared_vm::operation("netBlockListConstruct"), "BlockList")?;
    let prototype = get(c, block_list, "prototype")?;
    methods(c, prototype, &[
        ("addAddress", "netBlockListAddAddress"),
        ("addRange", "netBlockListAddRange"),
        ("addSubnet", "netBlockListAddSubnet"),
        ("check", "netBlockListCheck"),
        ("toJSON", "netBlockListToJson"),
        ("fromJSON", "netBlockListFromJson"),
    ])?;
    getters(c, prototype, &[("rules", "netBlockListRules")])?;
    let custom = c.host_function_named(crate::host::shared_vm::operation("netBlockListInspect"), "[nodejs.util.inspect.custom]")?;
    c.define_data_property_rooted(prototype, inspect, custom, true, false, true)?;
    methods(c, block_list, &[("isBlockList", "netIsBlockList")])?;
    define(c, holder, "BlockList", block_list)?;
    Ok(holder)
}

/// `net.BlockList` and `net.SocketAddress`.
pub(crate) fn classes(c: &mut Context<'_>) -> Result<(RootId, RootId), RootedError> {
    let holder = crate::host::shared_vm::commonjs::cached_native_binding(c, "\0net:block-list", build)?;
    Ok((get(c, holder, "BlockList")?, get(c, holder, "SocketAddress")?))
}

fn arg(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}

fn is_undefined(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|value| value.is_undefined())
}

fn require_new(c: &mut Context<'_>, name: &str) -> Result<(), RootedError> {
    if c.new_target_rooted().is_some() {
        return Ok(());
    }
    let error = c.type_error_rooted(&format!("Class constructor {name} cannot be invoked without 'new'"))?;
    Err(c.throw(error))
}

fn coded(c: &mut Context<'_>, code: &'static str, message: &str) -> RootedError {
    match c.error_rooted(message) {
        Ok(error) => match super::node_errors_shared_vm::decorate_error(c, error, code) {
            Ok(()) => c.throw(error),
            Err(failure) => failure,
        },
        Err(failure) => failure,
    }
}

fn string_argument(c: &mut Context<'_>, value: RootId, name: &str) -> Result<String, RootedError> {
    match c.string_text(value)? {
        Some(text) => Ok(text),
        None => Err(super::util_shared_vm::argument_type_error(c, value, name, "string")?),
    }
}

/// Node's `validatePort(port, name)`: a number or numeric string within 0..=65535.
fn validate_port(c: &mut Context<'_>, value: RootId, name: &str) -> Result<u16, RootedError> {
    let candidate = if let Some(number) = c.rooted_value(value).and_then(|value| value.as_number()) {
        Some(number)
    } else if let Some(text) = c.string_text(value)? {
        (!text.trim().is_empty()).then(|| c.to_number(value)).transpose()?
    } else {
        None
    };
    match candidate {
        Some(port) if port.fract() == 0.0 && (0.0..=MAX_PORT).contains(&port) => Ok(port as u16),
        _ => {
            let received = super::util_shared_vm::argument_type_description(c, value)?;
            let limit = u32::from(u16::MAX) + 1;
            let error = c.range_error_rooted(&format!("{name} should be >= 0 and < {limit}. Received {received}."))?;
            super::node_errors_shared_vm::decorate_range_error(c, error, "ERR_SOCKET_BAD_PORT")?;
            Err(c.throw(error))
        }
    }
}

fn option(c: &mut Context<'_>, options: RootId, name: &str) -> Outcome {
    get(c, options, name)
}

fn socket_address_from_options(c: &mut Context<'_>, options: RootId) -> Result<SocketAddress, RootedError> {
    if !c.is_object_rooted(options)? || c.is_callable_rooted(options)? || c.is_array_rooted(options)? {
        return Err(super::util_shared_vm::argument_type_error(c, options, "options", "Object")?);
    }
    let family_value = option(c, options, "family")?;
    let family_value = if is_undefined(c, family_value) { c.string_rooted(Family::V4.option()) } else { family_value };
    let family_text = c.string_text(family_value)?.map(|text| text.to_lowercase());
    let Some(family) = Family::ALL.into_iter().find(|family| family_text.as_deref() == Some(family.option())) else {
        return Err(super::util_shared_vm::argument_value_error(c, family_value, "options.family", "is invalid")?);
    };
    let address = option(c, options, "address")?;
    let address = if is_undefined(c, address) { c.string_rooted(family.default_address()) } else { address };
    let address = string_argument(c, address, "options.address")?;
    let port = option(c, options, "port")?;
    let port = if is_undefined(c, port) { c.number(0.0) } else { port };
    let flowlabel = option(c, options, "flowlabel")?;
    let flowlabel = if is_undefined(c, flowlabel) {
        0
    } else {
        let maximum = c.number(f64::from(u32::MAX));
        super::util_shared_vm::validate_integer(c, flowlabel, "options.flowlabel", 0.0, maximum)? as u32
    };
    let port = validate_port(c, port, "options.port")?;
    let Some(address) = family.parse(&address) else {
        return Err(coded(c, "ERR_INVALID_ADDRESS", "Invalid socket address"));
    };
    Ok(SocketAddress { family, address, port, flowlabel })
}

fn instantiate(c: &mut Context<'_>, address: SocketAddress) -> Outcome {
    let intrinsics = intrinsics(c)?;
    let prototype = get(c, intrinsics.socket_address, "prototype")?;
    let object = c.object_rooted()?;
    c.set_prototype_rooted(object, prototype)?;
    let record = c.uint8_array_rooted(&address.encode())?;
    c.define_data_property_rooted(object, intrinsics.socket_slot, record, false, false, false)?;
    Ok(object)
}

/// The record of a SocketAddress, if `value` is one.
fn socket_record(c: &mut Context<'_>, value: RootId) -> Result<Option<SocketAddress>, RootedError> {
    if !c.is_object_rooted(value)? {
        return Ok(None);
    }
    let slot = intrinsics(c)?.socket_slot;
    let record = c.get_property_rooted(value, slot)?;
    Ok(c.with_view_bytes_rooted(record, SocketAddress::decode))
}

fn this_socket(c: &mut Context<'_>, this: RootId) -> Result<SocketAddress, RootedError> {
    match socket_record(c, this)? {
        Some(address) => Ok(address),
        None => {
            let error = c.type_error_rooted("Illegal invocation")?;
            Err(c.throw(error))
        }
    }
}

pub(crate) fn socket_address_construct(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    require_new(c, "SocketAddress")?;
    let options = args.first().copied().filter(|options| !is_undefined(c, *options));
    let options = match options {
        Some(options) => options,
        None => c.object_rooted()?,
    };
    let address = socket_address_from_options(c, options)?;
    let slot = intrinsics(c)?.socket_slot;
    let record = c.uint8_array_rooted(&address.encode())?;
    c.define_data_property_rooted(this, slot, record, false, false, false)?;
    Ok(this)
}

pub(crate) fn socket_address_address(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let address = this_socket(c, this)?;
    Ok(c.string_rooted(&address.family.text(address.address)))
}

pub(crate) fn socket_address_port(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let address = this_socket(c, this)?;
    Ok(c.number(f64::from(address.port)))
}

pub(crate) fn socket_address_family(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let address = this_socket(c, this)?;
    Ok(c.string_rooted(address.family.option()))
}

pub(crate) fn socket_address_flowlabel(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let address = this_socket(c, this)?;
    Ok(c.number(f64::from(address.flowlabel)))
}

pub(crate) fn socket_address_to_json(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let address = this_socket(c, this)?;
    let json = c.object_rooted()?;
    let text = c.string_rooted(&address.family.text(address.address));
    c.assign_named_property_rooted(json, "address", text)?;
    let port = c.number(f64::from(address.port));
    c.assign_named_property_rooted(json, "port", port)?;
    let family = c.string_rooted(address.family.option());
    c.assign_named_property_rooted(json, "family", family)?;
    let flowlabel = c.number(f64::from(address.flowlabel));
    c.assign_named_property_rooted(json, "flowlabel", flowlabel)?;
    Ok(json)
}

/// `${name} ${inspect(value, { ...options, depth: options.depth - 1 })}`, or the object itself
/// below the inspection depth.
fn inspect_as(c: &mut Context<'_>, this: RootId, name: &str, value: RootId, args: &[RootId]) -> Outcome {
    let depth = arg(c, args, 0);
    if c.rooted_value(depth).and_then(|depth| depth.as_number()).is_some_and(|depth| depth < 0.0) {
        return Ok(this);
    }
    let options = arg(c, args, 1);
    let copy = c.object_rooted()?;
    if c.is_object_rooted(options)? {
        let global = c.global_root()?;
        let object = get(c, global, "Object")?;
        let assign = get(c, object, "assign")?;
        c.call_rooted(assign, object, &[copy, options])?;
        let depth = get(c, options, "depth")?;
        let depth = match c.rooted_value(depth).and_then(|depth| depth.as_number()) {
            Some(depth) => c.number(depth - 1.0),
            None => c.null(),
        };
        c.assign_named_property_rooted(copy, "depth", depth)?;
    }
    let inspect = super::util_shared_vm::internal_inspect(c)?;
    let undefined = c.undefined();
    let text = c.call_rooted(inspect, undefined, &[value, copy])?;
    let text = c.to_string(text)?;
    Ok(c.string_rooted(&format!("{name} {text}")))
}

pub(crate) fn socket_address_inspect(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let json = socket_address_to_json(c, this, &[])?;
    inspect_as(c, this, "SocketAddress", json, args)
}

pub(crate) fn is_socket_address(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let value = arg(c, args, 0);
    let is = socket_record(c, value)?.is_some();
    Ok(c.boolean(is))
}

/// `SocketAddress.parse('host:port')`: the address a URL authority names, or undefined.
pub(crate) fn socket_address_parse(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let input = arg(c, args, 0);
    let input = string_argument(c, input, "input")?;
    let (host, port) = match input.strip_prefix('[') {
        Some(rest) => match rest.split_once(']') {
            Some((host, tail)) => (host, tail.strip_prefix(':')),
            None => return Ok(c.undefined()),
        },
        None => match input.rsplit_once(':') {
            Some((host, port)) if !host.contains(':') => (host, Some(port)),
            _ => (input.as_str(), None),
        },
    };
    let family = if host.contains(':') { Family::V6 } else { Family::V4 };
    let port = match port {
        None | Some("") => 0,
        Some(port) => match port.parse::<u16>() {
            Ok(port) => port,
            Err(_) => return Ok(c.undefined()),
        },
    };
    match family.parse(host) {
        Some(address) => instantiate(c, SocketAddress { family, address, port, flowlabel: 0 }),
        None => Ok(c.undefined()),
    }
}

/// `address` (a SocketAddress, or text of `family`) as a SocketAddress record.
fn convert(c: &mut Context<'_>, address: RootId, family: RootId) -> Result<SocketAddress, RootedError> {
    if let Some(record) = socket_record(c, address)? {
        return Ok(record);
    }
    string_argument(c, address, "address")?;
    let family = if is_undefined(c, family) { c.string_rooted(Family::V4.option()) } else { family };
    string_argument(c, family, "family")?;
    let options = c.object_rooted()?;
    c.assign_named_property_rooted(options, "address", address)?;
    c.assign_named_property_rooted(options, "family", family)?;
    socket_address_from_options(c, options)
}

/// The rule table of a BlockList, if `value` is one.
fn rules_table(c: &mut Context<'_>, value: RootId) -> Result<Option<RootId>, RootedError> {
    if !c.is_object_rooted(value)? {
        return Ok(None);
    }
    let slot = intrinsics(c)?.rules_slot;
    let table = c.get_property_rooted(value, slot)?;
    Ok(c.with_view_bytes_rooted(table, |_| ()).map(|()| table))
}

fn rules_root(c: &mut Context<'_>, this: RootId) -> Outcome {
    if let Some(table) = rules_table(c, this)? {
        return Ok(table);
    }
    let error = c.type_error_rooted("Illegal invocation")?;
    Err(c.throw(error))
}

fn rules(c: &mut Context<'_>, this: RootId) -> Result<Vec<Rule>, RootedError> {
    let root = rules_root(c, this)?;
    Ok(c.with_view_bytes_rooted(root, |bytes| bytes.as_chunks::<RULE_RECORD>().0.iter().map(|record| Rule::decode(record)).collect())
        .unwrap_or_default())
}

/// Record `rule` as the list's newest.
fn insert(c: &mut Context<'_>, this: RootId, rule: Rule) -> Result<(), RootedError> {
    let root = rules_root(c, this)?;
    let mut bytes = Vec::with_capacity(RULE_RECORD);
    rule.encode(&mut bytes);
    c.with_view_bytes_rooted(root, |existing| bytes.extend_from_slice(existing));
    let slot = intrinsics(c)?.rules_slot;
    let table = c.uint8_array_from_rooted(bytes)?;
    c.define_data_property_rooted(this, slot, table, true, false, true)?;
    Ok(())
}

pub(crate) fn block_list_construct(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    require_new(c, "BlockList")?;
    let slot = intrinsics(c)?.rules_slot;
    let table = c.uint8_array_rooted(&[])?;
    c.define_data_property_rooted(this, slot, table, true, false, true)?;
    Ok(this)
}

pub(crate) fn add_address(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    rules_root(c, this)?;
    let (address, family) = (arg(c, args, 0), arg(c, args, 1));
    let address = convert(c, address, family)?;
    insert(c, this, Rule {
        kind: RuleKind::Address,
        family: address.family,
        prefix: address.family.bits(),
        start: address.address,
        end: address.address,
    })?;
    Ok(c.undefined())
}

pub(crate) fn add_range(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    rules_root(c, this)?;
    let (start_value, end_value, family) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    let start = convert(c, start_value, family)?;
    let end = convert(c, end_value, family)?;
    if start.address > end.address {
        let shown = instantiate(c, start)?;
        return Err(super::util_shared_vm::argument_value_error(c, shown, "start", "must come before end")?);
    }
    insert(c, this, Rule {
        kind: RuleKind::Range,
        family: start.family,
        prefix: start.family.bits(),
        start: start.address,
        end: end.address,
    })?;
    Ok(c.undefined())
}

pub(crate) fn add_subnet(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    rules_root(c, this)?;
    let (network, prefix, family) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    let network = convert(c, network, family)?;
    let maximum = c.number(f64::from(network.family.bits()));
    let prefix = super::util_shared_vm::validate_integer(c, prefix, "prefix", 0.0, maximum)? as u8;
    insert(c, this, Rule {
        kind: RuleKind::Subnet,
        family: network.family,
        prefix,
        start: network.address,
        end: network.address,
    })?;
    Ok(c.undefined())
}

pub(crate) fn check(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let root = rules_root(c, this)?;
    let (address, family) = (arg(c, args, 0), arg(c, args, 1));
    let address = match socket_record(c, address)? {
        Some(record) => record,
        None => {
            string_argument(c, address, "address")?;
            let family = if is_undefined(c, family) { c.string_rooted(Family::V4.option()) } else { family };
            string_argument(c, family, "family")?;
            let options = c.object_rooted()?;
            c.assign_named_property_rooted(options, "address", address)?;
            c.assign_named_property_rooted(options, "family", family)?;
            match super::stream_shared_vm::attempt(socket_address_from_options(c, options))? {
                Ok(record) => record,
                Err(_) => return Ok(c.boolean(false)),
            }
        }
    };
    let blocked = c
        .with_view_bytes_rooted(root, |bytes| {
            bytes.as_chunks::<RULE_RECORD>().0.iter().any(|record| Rule::decode(record).applies(address.address))
        })
        .unwrap_or(false);
    Ok(c.boolean(blocked))
}

pub(crate) fn rules_getter(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let rules = rules(c, this)?;
    let texts = rules.into_iter().map(|rule| c.string_rooted(&rule.text())).collect::<Vec<_>>();
    c.array_rooted(&texts)
}

pub(crate) fn block_list_inspect(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let rules = rules_getter(c, this, &[])?;
    let shown = c.object_rooted()?;
    c.assign_named_property_rooted(shown, "rules", rules)?;
    inspect_as(c, this, "BlockList", shown, args)
}

/// `new ERR_IP_BLOCKED(ip)`: a connection or datagram a block list refused.
pub(crate) fn ip_blocked(c: &mut Context<'_>, ip: &str) -> Outcome {
    let error = c.error_rooted(&format!("IP({ip}) is blocked by net.BlockList"))?;
    super::node_errors_shared_vm::decorate_error(c, error, "ERR_IP_BLOCKED")?;
    Ok(error)
}

pub(crate) fn is_block_list(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let value = arg(c, args, 0);
    let is = rules_table(c, value)?.is_some();
    Ok(c.boolean(is))
}

/// A rule's text (`'Subnet: IPv4 10.0.0.0/8'`) as the rule it names.
fn parse_rule(text: &str) -> Option<Rule> {
    let (kind, rest) = text.split_once(": ")?;
    let kind = RuleKind::ALL.into_iter().find(|candidate| candidate.label() == kind)?;
    let (family, data) = rest.split_once(' ')?;
    let family = Family::ALL.into_iter().find(|candidate| candidate.label() == family)?;
    let (start, end, prefix) = match kind {
        RuleKind::Address => (data, data, family.bits()),
        RuleKind::Range => {
            let (start, end) = data.split_once('-')?;
            (start, end, family.bits())
        }
        RuleKind::Subnet => {
            let (network, prefix) = data.rsplit_once('/')?;
            let prefix = prefix.parse::<u8>().ok().filter(|prefix| *prefix <= family.bits())?;
            (network, network, prefix)
        }
    };
    let (start, end) = (family.parse(start)?, family.parse(end)?);
    (start <= end).then_some(Rule { kind, family, prefix, start, end })
}

pub(crate) fn to_json(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    rules_getter(c, this, &[])
}

/// `fromJSON(rules)`: add each rule `toJSON` lists (a JSON text or an array of rule texts),
/// oldest first so the list reads back in the same order.
pub(crate) fn from_json(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    rules_root(c, this)?;
    let mut data = arg(c, args, 0);
    if c.string_text(data)?.is_some() {
        let global = c.global_root()?;
        let json = get(c, global, "JSON")?;
        let parse = get(c, json, "parse")?;
        data = c.call_rooted(parse, json, &[data])?;
    }
    if !c.is_array_rooted(data)? {
        return Err(super::util_shared_vm::argument_type_error(c, data, "data", "Array")?);
    }
    let entries = c.array_like_values_rooted(data)?;
    let mut parsed = Vec::with_capacity(entries.len());
    for entry in entries {
        let text = string_argument(c, entry, "data")?;
        parsed.extend(parse_rule(&text));
    }
    let existing = rules(c, this)?;
    for rule in parsed.into_iter().rev() {
        if !existing.contains(&rule) {
            insert(c, this, rule)?;
        }
    }
    Ok(c.undefined())
}
