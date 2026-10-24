//! Certificate name authorization shared by public TLS and encrypted sockets.
use crate::host::NodeHost;
use quench_runtime::{NativeArithmetic, NativeContext, RootId, RootedError};
use std::net::IpAddr;

fn matches_dns(host: &str, pattern: &str, wildcards: bool) -> bool {
    if pattern.is_empty() {
        return false;
    }
    let host: Vec<_> = host.split('.').collect();
    let pattern: Vec<_> = pattern.split('.').collect();
    if host.len() != pattern.len()
        || pattern
            .iter()
            .any(|part| part.is_empty() || part.chars().any(|c| !('!'..='\u{7f}').contains(&c)))
    {
        return false;
    }
    if host[1..] != pattern[1..] {
        return false;
    }
    let subdomain = pattern[0];
    if !subdomain.contains('*') || subdomain.contains("xn--") {
        return host[0] == subdomain;
    }
    const MIN_WILDCARD_DNS_LABELS: usize = 3;
    if !wildcards || pattern.len() < MIN_WILDCARD_DNS_LABELS {
        return false;
    }
    let mut pieces = subdomain.split('*');
    let prefix = pieces.next().unwrap();
    let suffix = pieces.next().unwrap();
    if pieces.next().is_some() {
        return false;
    }
    host[0].len() >= prefix.len() + suffix.len()
        && host[0].starts_with(prefix)
        && host[0].ends_with(suffix)
}

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}
fn put(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    context.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}
fn format_error(context: &mut NativeContext<'_, NodeHost>) -> RootedError {
    match context.syntax_error_rooted("Invalid subject alternative name string") {
        Ok(error) => {
            let code = context.string_rooted("ERR_TLS_CERT_ALTNAME_FORMAT");
            match put(context, error, "code", code) {
                Ok(()) => context.throw(error),
                Err(error) => error,
            }
        }
        Err(error) => error,
    }
}
// JSON string boundaries follow ECMA-404. Parsing uses the shared VM JSON owner.
fn quoted_end(text: &str) -> Option<usize> {
    let mut escaped = false;
    for (index, byte) in text.bytes().enumerate().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        match byte {
            b'\\' => escaped = true,
            b'"' => return Some(index + 1),
            _ => {}
        }
    }
    None
}
fn alt_names(
    context: &mut NativeContext<'_, NodeHost>,
    text: &str,
) -> Result<Vec<String>, RootedError> {
    if !text.contains('"') {
        return Ok(text.split(", ").map(str::to_owned).collect());
    }
    let mut names = Vec::new();
    let mut token = String::new();
    let mut offset = 0;
    while offset < text.len() {
        let tail = &text[offset..];
        let separator = tail.find(',');
        let quote = tail.find('"');
        if let Some(quote) =
            quote.filter(|quote| separator.is_none_or(|separator| *quote < separator))
        {
            token.push_str(&tail[..quote]);
            let quoted = &tail[quote..];
            let end = quoted_end(quoted).ok_or_else(|| format_error(context))?;
            let parsed = context
                .parse_json_rooted(&quoted[..end])
                .map_err(|_| format_error(context))?;
            token.push_str(
                &context
                    .string_text(parsed)?
                    .ok_or_else(|| format_error(context))?,
            );
            offset += quote + end;
        } else if let Some(separator) = separator {
            token.push_str(&tail[..separator]);
            names.push(std::mem::take(&mut token));
            // Node advances across the comma and one following character.
            offset += separator + 1;
            if offset < text.len() {
                offset += text[offset..].chars().next().unwrap().len_utf8();
            }
        } else {
            token.push_str(tail);
            offset = text.len();
        }
    }
    names.push(token);
    Ok(names)
}

pub(crate) fn check_server_identity(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let hostname = args.first().copied().unwrap_or_else(|| context.undefined());
    let cert = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let subject = get(context, cert, "subject")?;
    let alt = get(context, cert, "subjectaltname")?;
    let empty = context.string_rooted("");
    let hostname = context.arithmetic_rooted(NativeArithmetic::Add, empty, hostname)?;
    let original = context.to_string(hostname)?;
    let ascii = url::Host::parse(&original)
        .map(|host| host.to_string())
        .unwrap_or_default();
    let hostname_root = unfqdn_root(context, hostname)?;
    let normalized = context.to_string(hostname_root)?;
    let hostname = normalized.as_str();
    let ascii_root = context.string_rooted(&ascii);
    let ascii_root = unfqdn_root(context, ascii_root)?;
    let dns_array = context.array_rooted(&[])?;
    let ip_array = context.array_rooted(&[])?;
    if context.truthy_rooted(alt)? {
        let includes = get(context, alt, "includes")?;
        let quote = context.string_rooted("\"");
        let quoted = context.call_rooted(includes, alt, &[quote])?;
        let names = if context.truthy_rooted(quoted)? {
            let text = context.to_string(alt)?;
            let names = alt_names(context, &text)?;
            let names = names
                .iter()
                .map(|name| context.string_rooted(name))
                .collect::<Vec<_>>();
            context.array_rooted(&names)?
        } else {
            let split = get(context, alt, "split")?;
            let separator = context.string_rooted(", ");
            context.call_rooted(split, alt, &[separator])?
        };
        let for_each = get(context, names, "forEach")?;
        let data = context.array_rooted(&[dns_array, ip_array])?;
        let collect = context.host_function_with_data(
            crate::host::shared_vm::operation("tlsCollectAltName"),
            data,
        )?;
        context.call_rooted(for_each, names, &[collect])?;
    }
    let api = super::facade::install(context)?;
    let net = get(context, api, "identityNet")?;
    let is_ip_fn = get(context, net, "isIP")?;
    let hostname_value = context.string_rooted(hostname);
    let hostname_is_ip = context.call_rooted(is_ip_fn, net, &[hostname_value])?;
    let ascii_is_ip = context.call_rooted(is_ip_fn, net, &[ascii_root])?;
    if !context.truthy_rooted(hostname_is_ip)? && context.truthy_rooted(ascii_is_ip)? {
        let reason = "Host is a non-canonical IP address";
        let error = context.error_rooted(&format!(
            "Hostname/IP does not match certificate's altnames: {reason}"
        ))?;
        let code = context.string_rooted("ERR_TLS_CERT_ALTNAME_INVALID");
        put(context, error, "code", code)?;
        let reason = context.string_rooted(reason);
        put(context, error, "reason", reason)?;
        let hostname = context.string_rooted(hostname);
        put(context, error, "host", hostname)?;
        put(context, error, "cert", cert)?;
        return Ok(error);
    }
    let reason = if context.truthy_rooted(hostname_is_ip)? {
        let canonical = hostname
            .parse::<IpAddr>()
            .map(|ip| ip.to_string())
            .unwrap_or_default();
        let canonical = context.string_rooted(&canonical);
        let includes = get(context, ip_array, "includes")?;
        let valid = context.call_rooted(includes, ip_array, &[canonical])?;
        if context.truthy_rooted(valid)? {
            return Ok(context.undefined());
        }
        let join = get(context, ip_array, "join")?;
        let separator = context.string_rooted(", ");
        let ips = context.call_rooted(join, ip_array, &[separator])?;
        format!(
            "IP: {hostname} is not in the cert's list: {}",
            context.to_string(ips)?
        )
    } else {
        let length = get(context, dns_array, "length")?;
        let has_dns = context.truthy_rooted(length)?;
        let cn = if has_dns {
            context.undefined()
        } else if context
            .rooted_value(subject)
            .is_some_and(|value| value.is_null() || value.is_undefined())
        {
            context.undefined()
        } else {
            get(context, subject, "CN")?
        };
        if has_dns || context.truthy_rooted(cn)? {
            let host = pattern_text(context, ascii_root)?;
            let host = context.string_rooted(&host);
            let data = context.array_rooted(&[host])?;
            let matcher = context
                .host_function_with_data(crate::host::shared_vm::operation("tlsMatchName"), data)?;
            if has_dns {
                let some = get(context, dns_array, "some")?;
                let valid = context.call_rooted(some, dns_array, &[matcher])?;
                if context.truthy_rooted(valid)? {
                    return Ok(context.undefined());
                }
                format!(
                    "Host: {hostname}. is not in the cert's altnames: {}",
                    context.to_string(alt)?
                )
            } else {
                let cn = get(context, subject, "CN")?;
                let valid = if context.is_array_rooted(cn)? {
                    let some = get(context, cn, "some")?;
                    context.call_rooted(some, cn, &[matcher])?
                } else if context.truthy_rooted(cn)? {
                    let undefined = context.undefined();
                    context.call_rooted(matcher, undefined, &[cn])?
                } else {
                    context.boolean(false)
                };
                if context.truthy_rooted(valid)? {
                    return Ok(context.undefined());
                }
                format!(
                    "Host: {hostname}. is not cert's CN: {}",
                    context.to_string(cn)?
                )
            }
        } else {
            "Cert does not contain a DNS name".to_owned()
        }
    };
    let error = context.error_rooted(&format!(
        "Hostname/IP does not match certificate's altnames: {reason}"
    ))?;
    let code = context.string_rooted("ERR_TLS_CERT_ALTNAME_INVALID");
    put(context, error, "code", code)?;
    let reason = context.string_rooted(&reason);
    put(context, error, "reason", reason)?;
    let hostname = context.string_rooted(hostname);
    put(context, error, "host", hostname)?;
    put(context, error, "cert", cert)?;
    Ok(error)
}

fn array_strings(
    context: &mut NativeContext<'_, NodeHost>,
    array: RootId,
) -> Result<Vec<String>, RootedError> {
    let length = get(context, array, "length")?;
    let length = context
        .rooted_value(length)
        .and_then(|value| value.as_number())
        .unwrap_or(0.0) as usize;
    (0..length)
        .map(|index| {
            let item = get(context, array, &index.to_string())?;
            context.to_string(item)
        })
        .collect()
}

pub(crate) fn collect_alt_name(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let name = args.first().copied().unwrap_or_else(|| context.undefined());
    let data = context.host_function_data()?;
    for (prefix, offset, owner) in [
        ("DNS:", "DNS:".len(), "0"),
        ("IP Address:", "IP Address:".len(), "1"),
    ] {
        let starts_with = get(context, name, "startsWith")?;
        let prefix = context.string_rooted(prefix);
        let matched = context.call_rooted(starts_with, name, &[prefix])?;
        if !context.truthy_rooted(matched)? {
            continue;
        }
        let slice = get(context, name, "slice")?;
        let offset = context.number(offset as f64);
        let mut value = context.call_rooted(slice, name, &[offset])?;
        if owner == "1" {
            let text = context.to_string(value)?;
            let canonical = text
                .parse::<IpAddr>()
                .map(|ip| ip.to_string())
                .unwrap_or_default();
            value = context.string_rooted(&canonical);
        }
        let array = get(context, data, owner)?;
        let push = get(context, array, "push")?;
        context.call_rooted(push, array, &[value])?;
        break;
    }
    Ok(context.undefined())
}

fn pattern_text(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<String, RootedError> {
    let api = super::facade::install(context)?;
    let patterns = get(context, api, "identityPatterns")?;
    let upper_factory = get(context, patterns, "upperCase")?;
    let undefined = context.undefined();
    let upper_case = context.call_rooted(upper_factory, undefined, &[])?;
    let value = unfqdn_root(context, value)?;
    let replace = get(context, value, "replace")?;
    let lower = get(context, api, "identityLowercase")?;
    let value = context.call_rooted(replace, value, &[upper_case, lower])?;
    let split = get(context, value, "split")?;
    let dot = context.string_rooted(".");
    let parts = context.call_rooted(split, value, &[dot])?;
    Ok(array_strings(context, parts)?.join("."))
}
pub(crate) fn lowercase_name(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let char_code_at = get(context, value, "charCodeAt")?;
    let zero = context.number(0.0);
    let code = context.call_rooted(char_code_at, value, &[zero])?;
    const ASCII_CASE_OFFSET: u8 = b'a' - b'A';
    let offset = context.number(f64::from(ASCII_CASE_OFFSET));
    let code = context.arithmetic_rooted(NativeArithmetic::Add, offset, code)?;
    // ToUint16 and ToInt32 low sixteen bits coincide; existing VM arithmetic
    // owns original conversion hooks, BigInt/Symbol errors and numeric wrapping.
    let mask = context.number(f64::from(u16::MAX));
    let code = context.arithmetic_rooted(NativeArithmetic::BitwiseAnd, code, mask)?;
    let code = context
        .rooted_value(code)
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("character code conversion failed"))?
        as u16;
    Ok(context.string_units_rooted(&[code]))
}

fn unfqdn_root(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<RootId, RootedError> {
    let api = super::facade::install(context)?;
    let patterns = get(context, api, "identityPatterns")?;
    let trailing_factory = get(context, patterns, "trailingDot")?;
    let undefined = context.undefined();
    let trailing_dot = context.call_rooted(trailing_factory, undefined, &[])?;
    let replace = get(context, value, "replace")?;
    let empty = context.string_rooted("");
    context.call_rooted(replace, value, &[trailing_dot, empty])
}

pub(crate) fn match_name(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let pattern = args.first().copied().unwrap_or_else(|| context.undefined());
    if !context.truthy_rooted(pattern)? {
        return Ok(context.boolean(false));
    }
    let data = context.host_function_data()?;
    let host = get(context, data, "0")?;
    let host = context.to_string(host)?;
    let pattern = pattern_text(context, pattern)?;
    Ok(context.boolean(matches_dns(&host, &pattern, true)))
}
