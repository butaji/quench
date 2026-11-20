//! `node:os` in Rust: lib/os.js over the native OS binding on Unix, and the host facts
//! `sysinfo` reports elsewhere.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
#[cfg(not(target_os = "macos"))]
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[cfg(not(unix))]
pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    for name in [
        "type", "platform", "arch", "release", "hostname", "homedir", "tmpdir",
    ] {
        let data = context.string_rooted(name);
        let function =
            context.host_function_with_data(crate::host::shared_vm::operation!("osString"), data)?;
        set(context, module, name, function)?;
    }
    for (name, operation) in [
        ("totalmem", "osTotalmem"),
        ("uptime", "osUptime"),
        ("networkInterfaces", "osNetworkInterfaces"),
    ] {
        let function = context.host_function(crate::host::shared_vm::operation(operation))?;
        set(context, module, name, function)?;
    }
    set_string(
        context,
        module,
        "EOL",
        if cfg!(windows) { "\r\n" } else { "\n" },
    )?;
    set_string(
        context,
        module,
        "devNull",
        if cfg!(windows) {
            "\\.\\nul"
        } else {
            "/dev/null"
        },
    )?;
    Ok(module)
}

pub(crate) fn string_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let name = context.host_function_data()?;
    let name = context.string_text(name)?.unwrap_or_default();
    let value = match name.as_str() {
        "type" => crate::modules::os_facts::type_str(),
        "platform" => node_platform().to_owned(),
        "arch" => node_arch().to_owned(),
        #[cfg(not(target_os = "macos"))]
        "release" => sysinfo::System::kernel_version().unwrap_or_default(),
        #[cfg(not(target_os = "macos"))]
        "hostname" => sysinfo::System::host_name().unwrap_or_default(),
        #[cfg(target_os = "macos")]
        "release" => crate::modules::os_facts::system_text(c"kern.osrelease").unwrap_or_default(),
        #[cfg(target_os = "macos")]
        "hostname" => crate::modules::os_facts::system_text(c"kern.hostname").unwrap_or_default(),
        "homedir" => std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into_owned()),
        #[cfg(target_os = "linux")]
        "tmpdir" => crate::modules::os_facts::temporary_directory(),
        #[cfg(not(target_os = "linux"))]
        "tmpdir" => std::env::temp_dir().to_string_lossy().into_owned(),
        _ => String::new(),
    };
    Ok(context.string_rooted(&value))
}

pub(crate) fn totalmem_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    Ok(context.number(crate::modules::os_facts::total_memory_bytes() as f64))
}

pub(crate) fn uptime_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    #[cfg(not(target_os = "macos"))]
    let uptime = sysinfo::System::uptime();
    #[cfg(target_os = "macos")]
    let uptime = crate::modules::os_facts::coarse_uptime_seconds();
    Ok(context.number(uptime as f64))
}

pub(crate) fn network_interfaces_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    #[cfg(target_os = "macos")]
    { network_interfaces(context) }
    #[cfg(not(target_os = "macos"))]
    {
    let result = context.null_object_rooted()?;
    let networks = sysinfo::Networks::new_with_refreshed_list();
    for (name, network) in networks.list() {
        let mut addresses = Vec::new();
        for subnet in network.ip_networks() {
            let address = subnet.addr;
            let is_v4 = address.is_ipv4();
            let netmask = match address {
                IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::from(mask4(subnet.prefix))),
                IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::from(mask6(subnet.prefix))),
            };
            let entry = context.object_rooted()?;
            set_string(context, entry, "address", &address.to_string())?;
            set_string(
                context,
                entry,
                "family",
                if is_v4 { "IPv4" } else { "IPv6" },
            )?;
            set_string(context, entry, "netmask", &netmask.to_string())?;
            set_string(context, entry, "mac", &network.mac_address().to_string())?;
            set_bool(context, entry, "internal", address.is_loopback())?;
            set_string(
                context,
                entry,
                "cidr",
                &format!("{address}/{}", subnet.prefix),
            )?;
            set_number(context, entry, "scopeid", 0.0)?;
            addresses.push(entry);
        }
        let addresses = context.array_rooted(&addresses)?;
        set(context, result, name, addresses)?;
    }
    Ok(result)
    }
}

fn node_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        "linux" => "linux",
        other => other,
    }
}

fn node_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "ia32",
        "arm" => "arm",
        other => other,
    }
}

#[cfg(not(target_os = "macos"))]
fn mask4(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - u32::from(prefix.min(32)))
    }
}

#[cfg(not(target_os = "macos"))]
fn mask6(prefix: u8) -> u128 {
    if prefix == 0 {
        0
    } else {
        u128::MAX << (128 - u32::from(prefix.min(128)))
    }
}

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    if context.set_property_rooted(object, key, value, object)? {
        Ok(())
    } else {
        Err(RootedError::host(format!("cannot install os.{name}")))
    }
}

fn set_string(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: &str,
) -> Result<(), RootedError> {
    let value = context.string_rooted(value);
    set(context, object, name, value)
}

#[cfg(not(target_os = "macos"))]
fn set_bool(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: bool,
) -> Result<(), RootedError> {
    let value = context.boolean(value);
    set(context, object, name, value)
}

fn set_number(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: f64,
) -> Result<(), RootedError> {
    let value = context.number(value);
    set(context, object, name, value)
}

/// Public and captured-private projections derive from the OS signal owner.
#[cfg(unix)]
pub(crate) fn signal_constants(
    context: &mut NativeContext<'_, NodeHost>,
    inventory: &[(&str, libc::c_int)],
) -> Result<RootId, RootedError> {
    let signals = context.null_object_rooted()?;
    for (name, number) in inventory {
        let key = context.string_rooted(name);
        let value = context.number(f64::from(*number));
        context.define_data_property_rooted(signals, key, value, false, true, false)?;
    }
    if !context.prevent_extensions_rooted(signals)? {
        return Err(RootedError::host("cannot seal OS signal constants"));
    }
    Ok(signals)
}

pub(crate) fn freemem_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    Ok(context.number(crate::modules::os_facts::free_memory_bytes() as f64))
}

/// One `os` export that reads a host fact (lib/os.js), dispatched by `fact`.
#[cfg(unix)]
#[derive(Clone, Copy)]
enum Fact {
    Arch,
    AvailableParallelism,
    Cpus,
    Endianness,
    FreeMem,
    HomeDir,
    Hostname,
    LoadAvg,
    NetworkInterfaces,
    Platform,
    Release,
    TmpDir,
    TotalMem,
    Type,
    Uptime,
    Version,
    Machine,
}

/// An `os` function export: a fact read, under the function's own name and converting to
/// the fact (`[Symbol.toPrimitive]`) when `primitive`, or an operation taking arguments.
#[cfg(unix)]
#[derive(Clone, Copy)]
enum Export {
    Fact { name: &'static str, fact: Fact, primitive: bool },
    Operation { operation: &'static str, length: u8 },
}

/// The `os` function exports, in Node's order.
#[cfg(unix)]
const EXPORTS: &[(&str, Export)] = {
    use Export::{Fact as F, Operation as O};
    &[
        ("arch", F { name: "arch", fact: Fact::Arch, primitive: true }),
        ("availableParallelism", F { name: "", fact: Fact::AvailableParallelism, primitive: true }),
        ("cpus", F { name: "cpus", fact: Fact::Cpus, primitive: false }),
        ("endianness", F { name: "endianness", fact: Fact::Endianness, primitive: true }),
        ("freemem", F { name: "", fact: Fact::FreeMem, primitive: true }),
        ("getPriority", O { operation: "osGetPriority", length: 1 }),
        ("homedir", F { name: "wrappedFn", fact: Fact::HomeDir, primitive: true }),
        ("hostname", F { name: "wrappedFn", fact: Fact::Hostname, primitive: true }),
        ("loadavg", F { name: "loadavg", fact: Fact::LoadAvg, primitive: false }),
        ("networkInterfaces", F { name: "networkInterfaces", fact: Fact::NetworkInterfaces, primitive: false }),
        ("platform", F { name: "platform", fact: Fact::Platform, primitive: true }),
        ("release", F { name: "getOSRelease", fact: Fact::Release, primitive: true }),
        ("setPriority", O { operation: "osSetPriority", length: 2 }),
        ("tmpdir", F { name: "tmpdir", fact: Fact::TmpDir, primitive: true }),
        ("totalmem", F { name: "", fact: Fact::TotalMem, primitive: true }),
        ("type", F { name: "getOSType", fact: Fact::Type, primitive: true }),
        ("userInfo", O { operation: "osUserInfo", length: 1 }),
        ("uptime", F { name: "wrappedFn", fact: Fact::Uptime, primitive: true }),
        ("version", F { name: "getOSVersion", fact: Fact::Version, primitive: true }),
        ("machine", F { name: "getMachine", fact: Fact::Machine, primitive: true }),
    ]
};

/// `os.constants.priority`'s bounds, which `setPriority` validates against.
#[cfg(unix)]
const PRIORITY_HIGHEST: f64 = -20.0;
#[cfg(unix)]
const PRIORITY_LOW: f64 = 19.0;
/// `getInterfaceAddresses` reports each address as this many consecutive fields.
#[cfg(unix)]
pub(crate) const INTERFACE_FIELDS: usize = 7;

/// `node:os` (lib/os.js) over the native OS binding.
#[cfg(unix)]
pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    let global = context.global_root()?;
    let symbol = context.get_named_property_rooted(global, "Symbol")?;
    let to_primitive = context.get_named_property_rooted(symbol, "toPrimitive")?;
    for (index, &(export, kind)) in EXPORTS.iter().enumerate() {
        let function = match kind {
            Export::Operation { operation, length } => {
                let function = context.host_function_named(crate::host::shared_vm::operation(operation), export)?;
                let key = context.string_rooted("length");
                let length = context.number(f64::from(length));
                context.define_data_property_rooted(function, key, length, false, false, true)?;
                function
            }
            Export::Fact { name, primitive, .. } => {
                let data = context.number(index as f64);
                let operation = crate::host::shared_vm::operation!("osFact");
                let function = context.host_function_named_with_data(operation, name, data)?;
                if primitive {
                    let converter = context.host_function_named_with_data(operation, "", data)?;
                    context.set_property_rooted(function, to_primitive, converter, function)?;
                }
                function
            }
        };
        let key = context.string_rooted(export);
        context.define_data_property_rooted(module, key, function, true, true, true)?;
    }
    let constants = super::os_binding_shared_vm::constants(context)?;
    let key = context.string_rooted("constants");
    context.define_data_property_rooted(module, key, constants, false, true, false)?;
    for (name, value) in [("EOL", "\n"), ("devNull", "/dev/null")] {
        let key = context.string_rooted(name);
        let value = context.string_rooted(value);
        context.define_data_property_rooted(module, key, value, false, true, true)?;
    }
    Ok(module)
}

/// `getCheckedFunction`: a binding read that fills `ctx` and returns undefined on failure
/// throws `ERR_SYSTEM_ERROR`.
#[cfg(unix)]
pub(crate) fn checked(
    context: &mut NativeContext<'_, NodeHost>,
    read: fn(&mut NativeContext<'_, NodeHost>, RootId, &[RootId]) -> Result<RootId, RootedError>,
    leading: &[RootId],
) -> Result<RootId, RootedError> {
    let ctx = context.object_rooted()?;
    let mut args = leading.to_vec();
    args.push(ctx);
    let undefined = context.undefined();
    let value = read(context, undefined, &args)?;
    if context.rooted_value(value).is_some_and(|value| value.is_undefined()) {
        return Err(super::util_system_error_shared_vm::system_error(context, ctx)?);
    }
    Ok(value)
}

#[cfg(unix)]
fn process_property(context: &mut NativeContext<'_, NodeHost>, name: &str) -> Result<RootId, RootedError> {
    let process = context
        .host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process is not initialized"))?;
    context.get_named_property_rooted(process, name)
}

/// The OS information `getOSInformation` reports: type, version, release and machine.
#[cfg(unix)]
fn information(context: &mut NativeContext<'_, NodeHost>, field: usize) -> Result<RootId, RootedError> {
    let information = checked(context, super::os_binding_shared_vm::information, &[])?;
    context.get_named_property_rooted(information, &field.to_string())
}

/// An `os` fact export; its data is its `EXPORTS` index.
#[cfg(unix)]
pub(crate) fn fact(context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    use super::os_binding_shared_vm as binding;
    let data = context.host_function_data()?;
    let index = context.to_number(data)? as usize;
    let Some((_, Export::Fact { fact, .. })) = EXPORTS.get(index) else {
        return Err(RootedError::host("os fact export index out of range"));
    };
    let undefined = context.undefined();
    match fact {
        Fact::Arch => process_property(context, "arch"),
        Fact::Platform => process_property(context, "platform"),
        Fact::AvailableParallelism => binding::available_parallelism(context, undefined, &[]),
        Fact::FreeMem => freemem_operation(context, undefined, &[]),
        Fact::TotalMem => totalmem_operation(context, undefined, &[]),
        Fact::Endianness => Ok(context.string_rooted(if cfg!(target_endian = "big") { "BE" } else { "LE" })),
        Fact::HomeDir => checked(context, binding::home, &[]),
        Fact::Hostname => checked(context, binding::hostname, &[]),
        Fact::Uptime => checked(context, binding::uptime, &[]),
        Fact::TmpDir => binding::tempdir(context, undefined, &[]),
        Fact::Type => information(context, 0),
        Fact::Version => information(context, 1),
        Fact::Release => information(context, 2),
        Fact::Machine => information(context, 3),
        Fact::LoadAvg => {
            let zero = context.number(0.0);
            let averages = context.array_rooted(&[zero, zero, zero])?;
            binding::loadavg(context, undefined, &[averages])?;
            Ok(averages)
        }
        Fact::Cpus => cpus(context),
        Fact::NetworkInterfaces => network_interfaces(context),
    }
}

/// `os.cpus()`: `{ model, speed, times }` records from the binding's flat list.
#[cfg(unix)]
fn cpus(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let mut result = Vec::new();
    for cpu in crate::modules::os_facts::cpus() {
        let object = context.object_rooted()?;
        set_string(context, object, "model", &cpu.model)?;
        set_number(context, object, "speed", cpu.speed as f64)?;
        let times = context.object_rooted()?;
        for (name, time) in [
            ("user", cpu.times.user),
            ("nice", cpu.times.nice),
            ("sys", cpu.times.sys),
            ("idle", cpu.times.idle),
            ("irq", cpu.times.irq),
        ] {
            set_number(context, times, name, time as f64)?;
        }
        set(context, object, "times", times)?;
        result.push(object);
    }
    context.array_rooted(&result)
}

/// `os.networkInterfaces()`: the binding's seven-field records grouped by interface name.
#[cfg(unix)]
fn network_interfaces(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let records = checked(context, super::os_binding_shared_vm::interfaces, &[])?;
    let fields = context.array_like_values_rooted(records)?;
    let result = context.object_rooted()?;
    for [name, address, netmask, family, mac, internal, scope] in fields.as_chunks::<INTERFACE_FIELDS>().0 {
        let entry = context.object_rooted()?;
        for (key, value) in [("address", *address), ("netmask", *netmask), ("family", *family), ("mac", *mac), ("internal", *internal)] {
            set(context, entry, key, value)?;
        }
        let address_text = context.to_string(*address)?;
        let netmask_text = context.to_string(*netmask)?;
        let family_text = context.to_string(*family)?;
        let cidr = match cidr(&address_text, &netmask_text, &family_text) {
            Some(cidr) => context.string_rooted(&cidr),
            None => context.null(),
        };
        set(context, entry, "cidr", cidr)?;
        if context.to_number(*scope)? != NO_SCOPE {
            set(context, entry, "scopeid", *scope)?;
        }
        let existing = context.get_property_rooted(result, *name)?;
        if context.is_array_rooted(existing)? {
            let length = context.get_named_property_rooted(existing, "length")?;
            context.set_property_rooted(existing, length, entry, existing)?;
        } else {
            let list = context.array_rooted(&[entry])?;
            context.set_property_rooted(result, *name, list, result)?;
        }
    }
    Ok(result)
}

/// `getInterfaceAddresses` marks an address without an IPv6 scope with this scope.
#[cfg(unix)]
pub(crate) const NO_SCOPE: f64 = -1.0;

/// `getCIDR`: the prefix length of a contiguous netmask, `None` for any other mask.
#[cfg(unix)]
fn cidr(address: &str, netmask: &str, family: &str) -> Option<String> {
    let (separator, radix, group_bits) = if family == "IPv6" { (':', 16, 16) } else { ('.', 10, 8) };
    let mut ones = 0;
    let mut zeros = false;
    for part in netmask.split(separator).filter(|part| !part.is_empty()) {
        if zeros {
            if part != "0" {
                return None;
            }
            continue;
        }
        let bits = u32::from_str_radix(part, radix).ok()?;
        let count = bits.count_ones();
        ones += count;
        if count != group_bits {
            if format!("{bits:b}").contains("01") {
                return None;
            }
            zeros = true;
        }
    }
    Some(format!("{address}/{ones}"))
}

/// `validateInt32(value, name, min, max)`.
#[cfg(unix)]
fn validate_int32(context: &mut NativeContext<'_, NodeHost>, value: RootId, name: &str, minimum: f64, maximum: f64) -> Result<f64, RootedError> {
    let maximum = context.number(maximum);
    super::util_shared_vm::validate_integer(context, value, name, minimum, maximum)
}

/// `os.getPriority(pid)`.
#[cfg(unix)]
pub(crate) fn get_priority(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let pid = match args.first().copied().filter(|pid| !context.rooted_value(*pid).is_some_and(|value| value.is_undefined())) {
        Some(pid) => {
            validate_int32(context, pid, "pid", f64::from(i32::MIN), f64::from(i32::MAX))?;
            pid
        }
        None => context.number(0.0),
    };
    checked(context, super::os_binding_shared_vm::get_priority, &[pid])
}

/// `os.setPriority([pid, ]priority)`.
#[cfg(unix)]
pub(crate) fn set_priority(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let first = args.first().copied().unwrap_or(undefined);
    let second = args.get(1).copied().unwrap_or(undefined);
    let (pid, priority) = if context.rooted_value(second).is_some_and(|value| value.is_undefined()) {
        (context.number(0.0), first)
    } else {
        (first, second)
    };
    validate_int32(context, pid, "pid", f64::from(i32::MIN), f64::from(i32::MAX))?;
    validate_int32(context, priority, "priority", PRIORITY_HIGHEST, PRIORITY_LOW)?;
    let ctx = context.object_rooted()?;
    let status = super::os_binding_shared_vm::set_priority(context, undefined, &[pid, priority, ctx])?;
    if context.to_number(status)? != 0.0 {
        return Err(super::util_system_error_shared_vm::system_error(context, ctx)?);
    }
    Ok(undefined)
}

/// `os.userInfo(options)`.
#[cfg(unix)]
pub(crate) fn user_info(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let options = args.first().copied().unwrap_or_else(|| context.undefined());
    let options = if context.value_type_rooted(options) == Some(quench_runtime::ValueType::Object) { options } else { context.null() };
    checked(context, super::os_binding_shared_vm::user_info, &[options])
}
