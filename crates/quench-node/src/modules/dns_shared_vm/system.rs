//! The hosts file and lookup order c-ares consults for reverse lookups, and libuv's
//! address text forms.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

const HOSTS_FILE: &str = "/etc/hosts";
const NSSWITCH_FILE: &str = "/etc/nsswitch.conf";

/// The libuv error code for a named error (`UV_<name>`).
pub(crate) fn uv_code(name: &str) -> i32 {
    crate::modules::util_system_error_shared_vm::UV_ERRORS
        .iter()
        .find(|row| row.1 == name)
        .map(|row| row.0)
        .expect("libuv defines every translated error")
}

/// `uv_inet_pton`: dotted-quad IPv4, or IPv6 with any `%scope` suffix ignored.
pub(crate) fn parse_ip(text: &str) -> Option<IpAddr> {
    if let Ok(address) = text.parse::<Ipv4Addr>() {
        return Some(IpAddr::V4(address));
    }
    let address = text.split_once('%').map_or(text, |(address, _)| address);
    address.parse::<Ipv6Addr>().ok().map(IpAddr::V6)
}

/// `uv_inet_ntop`.
pub(crate) fn ip_text(address: IpAddr) -> String {
    match address {
        IpAddr::V4(address) => address.to_string(),
        IpAddr::V6(address) => ipv6_text(address),
    }
}

/// libuv's `inet_ntop6`: the longest run of two or more zero groups compresses (first
/// wins on ties), and a leading run of six or five-then-ffff zero groups prints the
/// trailing 32 bits as IPv4.
pub(crate) fn ipv6_text(address: Ipv6Addr) -> String {
    const EMBEDDED_IPV4_GROUP: usize = 6;
    const COMPATIBLE_RUN: usize = 6;
    const MAPPED_RUN: usize = 5;
    const MAPPED_MARKER: u16 = 0xffff;
    let words = address.segments();
    let (mut best, mut current): (Option<(usize, usize)>, Option<(usize, usize)>) = (None, None);
    for (index, &word) in words.iter().enumerate() {
        if word == 0 {
            current = Some(current.map_or((index, 1), |(base, length)| (base, length + 1)));
        } else if let Some(run) = current.take() {
            if best.is_none_or(|best| run.1 > best.1) {
                best = Some(run);
            }
        }
    }
    if let Some(run) = current {
        if best.is_none_or(|best| run.1 > best.1) {
            best = Some(run);
        }
    }
    let best = best.filter(|run| run.1 >= 2);
    let mut text = String::new();
    for (index, &word) in words.iter().enumerate() {
        if let Some((base, length)) = best {
            if (base..base + length).contains(&index) {
                if index == base {
                    text.push(':');
                }
                continue;
            }
        }
        if index != 0 {
            text.push(':');
        }
        let embedded = best.is_some_and(|(base, length)| {
            base == 0 && (length == COMPATIBLE_RUN || (length == MAPPED_RUN && words[MAPPED_RUN] == MAPPED_MARKER))
        });
        if index == EMBEDDED_IPV4_GROUP && embedded {
            let octets = address.octets();
            text.push_str(&Ipv4Addr::new(octets[12], octets[13], octets[14], octets[15]).to_string());
            return text;
        }
        text.push_str(&format!("{word:x}"));
    }
    if best.is_some_and(|(base, length)| base + length == words.len()) {
        text.push(':');
    }
    text
}

/// One merged hosts-file entry: c-ares joins lines that share an address or a hostname.
struct HostsEntry {
    addresses: Vec<IpAddr>,
    names: Vec<String>,
}

fn hosts_entries() -> Vec<HostsEntry> {
    let Ok(text) = std::fs::read_to_string(HOSTS_FILE) else { return Vec::new() };
    let mut entries: Vec<HostsEntry> = Vec::new();
    let mut by_address: Vec<(IpAddr, usize)> = Vec::new();
    let mut by_name: Vec<(String, usize)> = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or_default();
        let mut words = line.split_whitespace();
        let Some(address) = words.next().and_then(parse_ip) else { continue };
        let mut names: Vec<String> = Vec::new();
        for name in words {
            let valid = name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_'));
            if valid && !names.iter().any(|seen| seen.eq_ignore_ascii_case(name)) {
                names.push(name.to_owned());
            }
        }
        if names.is_empty() {
            continue;
        }
        let known = |name: &str, by_name: &[(String, usize)]| by_name.iter().find(|row| row.0.eq_ignore_ascii_case(name)).map(|row| row.1);
        let matched_address = by_address.iter().find(|row| row.0 == address).map(|row| row.1);
        let matched = matched_address.or_else(|| names.iter().find_map(|name| known(name, &by_name)));
        let index = match matched {
            Some(index) => {
                if matched_address.is_none() {
                    entries[index].addresses.push(address);
                    by_address.push((address, index));
                }
                for name in names {
                    if known(&name, &by_name).is_none() {
                        entries[index].names.push(name.clone());
                        by_name.push((name, index));
                    }
                }
                continue;
            }
            None => entries.len(),
        };
        by_address.push((address, index));
        for name in &names {
            if known(name, &by_name).is_none() {
                by_name.push((name.clone(), index));
            }
        }
        entries.push(HostsEntry { addresses: vec![address], names });
    }
    entries
}

/// A reverse lookup answered by the hosts file: the entry's aliases (every name after
/// its first), as c-ares' hostent conversion reports them.
pub(crate) fn hosts_reverse(address: IpAddr) -> Option<Vec<String>> {
    hosts_entries()
        .into_iter()
        .find(|entry| entry.addresses.contains(&address))
        .map(|entry| entry.names.into_iter().skip(1).collect())
}

/// Where reverse lookups look, in order (`f`ile and `b`ind, c-ares' `lookups`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    Hosts,
    Dns,
}

/// The hosts line of nsswitch.conf, with c-ares' service names; c-ares defaults to the
/// hosts file, then DNS.
pub(crate) fn lookup_order() -> Vec<Source> {
    const DEFAULT: [Source; 2] = [Source::Hosts, Source::Dns];
    const DNS_SERVICES: [&str; 6] = ["dns", "bind", "bind4", "bind6", "resolv", "resolve"];
    const FILE_SERVICES: [&str; 5] = ["files", "file", "local", "local4", "local6"];
    let Ok(text) = std::fs::read_to_string(NSSWITCH_FILE) else { return DEFAULT.to_vec() };
    let Some(services) = text.lines().find_map(|line| line.trim_start().strip_prefix("hosts:")) else {
        return DEFAULT.to_vec();
    };
    let mut order = Vec::new();
    for service in services.split_whitespace() {
        let is = |names: &[&str]| names.iter().any(|name| name.eq_ignore_ascii_case(service));
        let source = if is(&DNS_SERVICES) {
            Source::Dns
        } else if is(&FILE_SERVICES) {
            Source::Hosts
        } else {
            continue;
        };
        if !order.contains(&source) {
            order.push(source);
        }
    }
    if order.is_empty() { DEFAULT.to_vec() } else { order }
}

/// The reverse-lookup name for an address (`in-addr.arpa` or nibble `ip6.arpa`).
pub(crate) fn reverse_name(address: IpAddr) -> String {
    match address {
        IpAddr::V4(address) => {
            let [a, b, c, d] = address.octets();
            format!("{d}.{c}.{b}.{a}.in-addr.arpa")
        }
        IpAddr::V6(address) => {
            let mut name = String::new();
            for byte in address.octets().iter().rev() {
                name.push_str(&format!("{:x}.{:x}.", byte & 0xf, byte >> 4));
            }
            name.push_str("ip6.arpa");
            name
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv6_text_matches_libuv() {
        for (input, expected) in [
            ("::1", "::1"),
            ("::", "::"),
            ("2001:db8::1", "2001:db8::1"),
            ("::ffff:1.2.3.4", "::ffff:1.2.3.4"),
            ("::1.2.3.4", "::1.2.3.4"),
            ("1:0:0:2:0:0:0:3", "1:0:0:2::3"),
            ("1:0:2:3:4:5:6:7", "1:0:2:3:4:5:6:7"),
            ("fe80::", "fe80::"),
        ] {
            assert_eq!(ipv6_text(input.parse().unwrap()), expected);
        }
    }

    #[test]
    fn reverse_names_use_nibbles() {
        assert_eq!(reverse_name("192.0.2.1".parse().unwrap()), "1.2.0.192.in-addr.arpa");
        assert!(reverse_name("2001:db8::1".parse().unwrap()).starts_with("1.0.0.0.0.0.0.0"));
    }
}
