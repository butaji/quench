//! Host-backed implementation of Node's commonly used `os` APIs.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

#[cfg(not(unix))]
pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let module = context.object_rooted()?;
    for name in [
        "type", "platform", "arch", "release", "hostname", "homedir", "tmpdir",
    ] {
        let data = context.string_rooted(name);
        let function =
            context.host_function_with_data(crate::host::shared_vm::operation("osString"), data)?;
        set(context, module, name, function)?;
    }
    for (name, operation) in [
        #[cfg(not(target_os = "linux"))]
        ("totalmem", "osTotalmem"),
        ("uptime", "osUptime"),
        ("networkInterfaces", "osNetworkInterfaces"),
    ] {
        let function = context.host_function(crate::host::shared_vm::operation(operation))?;
        set(context, module, name, function)?;
    }
    #[cfg(target_os = "linux")]
    {
        let cpus = context.host_constructor(crate::host::shared_vm::operation("osCpus"))?;
        let key = context.string_rooted("name");
        let name = context.string_rooted("cpus");
        context.define_data_property_rooted(cpus, key, name, false, false, true)?;
        set(context, module, "cpus", cpus)?;
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
    #[cfg(target_os = "linux")]
    {
        let constants = context.null_object_rooted()?;
        let signals = signal_constants(context, super::process_signals_state::SIGNALS)?;
        set(context, constants, "signals", signals)?;
        set(context, module, "constants", constants)?;
    }
    Ok(module)
}

pub(crate) fn cpus_operation(context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    #[cfg(target_os = "linux")]
    {
        let mut result = Vec::new();
        for cpu in crate::modules::os_facts::cpus() {
            let object = context.object_rooted()?;
            set_string(context, object, "model", &cpu.model)?;
            set_number(context, object, "speed", cpu.speed as f64)?;
            let times = context.object_rooted()?;
            for (name, time) in [("user", cpu.times.user), ("nice", cpu.times.nice), ("sys", cpu.times.sys), ("idle", cpu.times.idle), ("irq", cpu.times.irq)] {
                set_number(context, times, name, time as f64)?;
            }
            set(context, object, "times", times)?;
            result.push(object);
        }
        context.array_rooted(&result)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = context;
        Err(RootedError::host("CPU facts are not implemented on this platform"))
    }
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
        "release" => sysinfo::System::kernel_version().unwrap_or_default(),
        "hostname" => sysinfo::System::host_name().unwrap_or_default(),
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
    Ok(context.number(sysinfo::System::uptime() as f64))
}

pub(crate) fn network_interfaces_operation(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
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

fn mask4(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - u32::from(prefix.min(32)))
    }
}

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
