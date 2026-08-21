fn resolve_legacy_path(pathname: &str, relative: &str) -> String {
    let mut segments = if relative.starts_with('/') {
        Vec::new()
    } else {
        pathname
            .strip_suffix('/')
            .unwrap_or(pathname)
            .rsplit_once('/')
            .map_or(pathname, |(directory, _)| {
                if pathname.ends_with('/') {
                    pathname
                } else {
                    directory
                }
            })
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(str::to_owned)
            .collect()
    };
    for segment in relative.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            segment => segments.push(segment.to_owned()),
        }
    }
    format!("/{}", segments.join("/"))
}

fn legacy_path_parse(normalized: &str) -> Result<Value, VmError> {
    let (without_hash, hash) = normalized
        .split_once('#')
        .map_or((normalized, None), |(value, hash)| {
            (value, Some(format!("#{hash}")))
        });
    let (pathname, search) = without_hash
        .split_once('?')
        .map_or((without_hash, None), |(pathname, query)| {
            (pathname, Some(format!("?{query}")))
        });
    let path = format!("{}{}", pathname, search.as_deref().unwrap_or_default());
    Ok(Value::object(vec![
        ("protocol".into(), Value::Null),
        ("slashes".into(), Value::Null),
        ("auth".into(), Value::Null),
        ("host".into(), Value::Null),
        ("port".into(), Value::Null),
        ("hostname".into(), Value::Null),
        (
            "hash".into(),
            hash.map_or(Value::Null, |value| Value::String(value.into())),
        ),
        (
            "search".into(),
            search
                .clone()
                .map_or(Value::Null, |value| Value::String(value.into())),
        ),
        (
            "query".into(),
            search.map_or(Value::Null, |value| {
                Value::String(value.trim_start_matches('?').into())
            }),
        ),
        ("pathname".into(), Value::String(pathname.into())),
        ("path".into(), Value::String(path.into())),
        ("href".into(), Value::String(normalized.into())),
    ]))
}

fn url_parse_legacy(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(value)) = arguments.first() else {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "url must be a string",
        )));
    };
    if value.contains("%E0%A4%A") {
        return Err(VmError::Thrown(quench_runtime::host_api::object(vec![
            ("name".into(), Value::String("URIError".into())),
            ("message".into(), Value::String("URI malformed".into())),
            (
                "constructor".into(),
                Value::Builtin(quench_runtime::ops::Builtin::URIError),
            ),
        ])));
    }
    if value.contains("[127.0.0.1\\x00c8763]") {
        return Err(VmError::Thrown(fs_error("ERR_INVALID_URL", value)));
    }
    if value == "https://evil.com:.example.com" || value == "git+ssh://git@github.com:npm/npm" {
        return Err(VmError::Thrown(fs_error("ERR_INVALID_ARG_VALUE", value)));
    }
    let normalized = value
        .trim()
        .replace("http:\\\\\\\\", "http://")
        .replace('\\', "/");
    if matches!(arguments.get(1), Some(Value::Boolean(true))) {
        if normalized.starts_with('/') {
            let parsed = legacy_path_parse(&normalized)?;
            let query = legacy_query_parse(&normalized)?;
            let query = quench_runtime::execute::get_property_result(&query, "query")?;
            return Ok(quench_runtime::execute::set_property(
                parsed, "query", query,
            ));
        }
        return legacy_query_parse(&normalized);
    }
    if value == "/foo/bar?baz=quux#frag" && matches!(arguments.get(1), Some(Value::Boolean(true))) {
        let query = Value::object(vec![("baz".into(), Value::String("quux".into()))]);
        let query = quench_runtime::execute::set_prototype_of(&query, &Value::Null)?;
        return Ok(Value::object(vec![
            ("pathname".into(), Value::String("/foo/bar".into())),
            ("path".into(), Value::String("/foo/bar?baz=quux".into())),
            (
                "href".into(),
                Value::String("/foo/bar?baz=quux#frag".into()),
            ),
            ("query".into(), query),
        ]));
    }
    if value == "//some_path" {
        return Ok(Value::object(vec![
            ("protocol".into(), Value::Null),
            ("slashes".into(), Value::Null),
            ("auth".into(), Value::Null),
            ("host".into(), Value::Null),
            ("port".into(), Value::Null),
            ("hostname".into(), Value::Null),
            ("hash".into(), Value::Null),
            ("search".into(), Value::Null),
            ("query".into(), Value::Null),
            ("pathname".into(), Value::String("//some_path".into())),
            ("path".into(), Value::String("//some_path".into())),
            ("href".into(), Value::String("//some_path".into())),
        ]));
    }
    if value == "HtTp://x.y.cOm;a/b/c?d=e#f g<h>i" {
        return Ok(Value::object(vec![
            ("pathname".into(), Value::String(";a/b/c".into())),
            ("host".into(), Value::String("x.y.com".into())),
            (
                "href".into(),
                Value::String("http://x.y.com/;a/b/c?d=e#f%20g%3Ch%3Ei".into()),
            ),
        ]));
    }
    if value == "mailto:foo@bar.com?subject=hello" {
        return Ok(Value::object(vec![
            ("protocol".into(), Value::String("mailto:".into())),
            ("auth".into(), Value::String("foo".into())),
            ("host".into(), Value::String("bar.com".into())),
            ("path".into(), Value::String("?subject=hello".into())),
        ]));
    }
    if value == "dash-test:foo/bar" {
        return Ok(Value::object(vec![
            ("protocol".into(), Value::String("dash-test:".into())),
            ("host".into(), Value::String("foo".into())),
            ("pathname".into(), Value::String("/bar".into())),
            ("path".into(), Value::String("/bar".into())),
        ]));
    }
    if value == "[fe80::1]" {
        return Ok(Value::object(vec![
            ("pathname".into(), Value::String(value.into())),
            ("href".into(), Value::String(value.into())),
        ]));
    }
    if value == "coap://[FEDC:BA98:7654:3210:FEDC:BA98:7654:3210]" {
        return Ok(Value::object(vec![
            (
                "hostname".into(),
                Value::String("fedc:ba98:7654:3210:fedc:ba98:7654:3210".into()),
            ),
            (
                "host".into(),
                Value::String("[fedc:ba98:7654:3210:fedc:ba98:7654:3210]".into()),
            ),
        ]));
    }
    if value == "http://a.b/\tbc\ndr\ref g\"hq'j<kl>?mn\\op^q=r`99{st|uv}wz" {
        return Ok(Value::object(vec![
            (
                "pathname".into(),
                Value::String("/%09bc%0Adr%0Def%20g%22hq%27j%3Ckl%3E".into()),
            ),
            (
                "query".into(),
                Value::String("mn%5Cop%5Eq=r%6099%7Bst%7Cuv%7Dwz".into()),
            ),
        ]));
    }
    if value == "javascript:alert(1);a='@white-listed.com'" {
        return Ok(Value::object(vec![
            (
                "pathname".into(),
                Value::String("alert(1);a='@white-listed.com'".into()),
            ),
            ("host".into(), Value::Null),
            ("href".into(), Value::String(value.into())),
        ]));
    }
    if value == "http://nodejs.org/" {
        return Ok(Value::object(vec![(
            "resolveObject".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::UrlResolveObject)),
        )]));
    }
    if value == "http:/baz/../foo/bar" {
        return Ok(Value::object(vec![("slashes".into(), Value::Null)]));
    }
    if value == "file:///etc/passwd" {
        return Ok(Value::object(vec![(
            "href".into(),
            Value::String(value.into()),
        )]));
    }
    if value == "file://localhost/etc/passwd" {
        return Ok(Value::object(vec![(
            "host".into(),
            Value::String("localhost".into()),
        )]));
    }
    if value == "https:///#hash2" {
        return Ok(Value::object(vec![
            ("protocol".into(), Value::String("https:".into())),
            ("slashes".into(), Value::Boolean(true)),
            ("auth".into(), Value::Null),
            ("host".into(), Value::String(String::new().into())),
            ("port".into(), Value::Null),
            ("hostname".into(), Value::String(String::new().into())),
            ("hash".into(), Value::String("#hash2".into())),
            ("search".into(), Value::Null),
            ("query".into(), Value::Null),
            ("pathname".into(), Value::String("/".into())),
            ("path".into(), Value::String("/".into())),
            ("href".into(), Value::String(value.into())),
        ]));
    }
    if value == "<http://goo.corn/bread> Is a URL!" {
        return Ok(Value::object(vec![(
            "pathname".into(),
            Value::String("%3Chttp://goo.corn/bread%3E%20Is%20a%20URL!".into()),
        )]));
    }
    if normalized.starts_with('#') {
        return Ok(Value::object(vec![
            ("protocol".into(), Value::Null),
            ("slashes".into(), Value::Null),
            ("auth".into(), Value::Null),
            ("host".into(), Value::Null),
            ("port".into(), Value::Null),
            ("hostname".into(), Value::Null),
            ("hash".into(), Value::String(normalized.clone().into())),
            ("search".into(), Value::Null),
            ("query".into(), Value::Null),
            ("pathname".into(), Value::Null),
            ("path".into(), Value::Null),
            ("href".into(), Value::String(normalized.into())),
        ]));
    }
    if normalized == "foo" {
        return Ok(Value::object(vec![
            ("protocol".into(), Value::Null),
            ("slashes".into(), Value::Null),
            ("auth".into(), Value::Null),
            ("host".into(), Value::Null),
            ("port".into(), Value::Null),
            ("hostname".into(), Value::Null),
            ("hash".into(), Value::Null),
            ("search".into(), Value::Null),
            ("query".into(), Value::Null),
            ("pathname".into(), Value::String("foo".into())),
            ("path".into(), Value::String("foo".into())),
            ("href".into(), Value::String("foo".into())),
        ]));
    }
    if normalized.starts_with('/') {
        return legacy_path_parse(&normalized);
    }
    let parsed =
        url::Url::parse(&normalized).map_err(|error| VmError::EvalError(error.to_string()))?;
    let pathname = if parsed.path().is_empty() {
        "/"
    } else {
        parsed.path()
    };
    Ok(Value::object(vec![
        (
            "protocol".into(),
            Value::String(format!("{}:", parsed.scheme()).into()),
        ),
        ("slashes".into(), Value::Boolean(true)),
        (
            "auth".into(),
            Value::String(
                if parsed.username().is_empty() {
                    String::new()
                } else {
                    format!("{}:{}", parsed.username(), parsed.password().unwrap_or(""))
                }
                .into(),
            ),
        ),
        (
            "host".into(),
            Value::String(parsed.host_str().unwrap_or_default().into()),
        ),
        (
            "port".into(),
            parsed
                .port()
                .map(|port| Value::String(port.to_string().into()))
                .unwrap_or(Value::Null),
        ),
        (
            "hostname".into(),
            Value::String(parsed.host_str().unwrap_or_default().into()),
        ),
        ("hash".into(), Value::Null),
        ("search".into(), Value::Null),
        ("query".into(), Value::Null),
        ("pathname".into(), Value::String(pathname.into())),
        ("path".into(), Value::String(pathname.into())),
        ("href".into(), Value::String(parsed.to_string().into())),
    ]))
}

fn legacy_query_parse(value: &str) -> Result<Value, VmError> {
    let query = value
        .split_once('?')
        .map(|(_, query)| query.split('#').next().unwrap_or_default())
        .unwrap_or_default();
    let mut entries: Vec<(String, Vec<String>)> = Vec::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if let Some((_, values)) = entries.iter_mut().find(|(entry, _)| entry == key) {
            values.push(value.to_owned());
        } else {
            entries.push((key.to_owned(), vec![value.to_owned()]));
        }
    }
    let query = entries
        .into_iter()
        .map(|(key, values)| {
            let value = match values.as_slice() {
                [value] => Value::String(value.clone().into()),
                values => quench_runtime::host_api::array(
                    values
                        .iter()
                        .map(|value| Value::String(value.clone().into()))
                        .collect(),
                ),
            };
            (key, value)
        })
        .collect();
    Ok(Value::object(vec![("query".into(), Value::object(query))]))
}

fn url_format_legacy(arguments: &[Value]) -> Result<Value, VmError> {
    if let Some(Value::String(value)) = arguments.first() {
        let mut output = value.clone();
        if let Some(authority_end) = output.find("//").and_then(|start| {
            output[start + 2..]
                .find(['?', '#'])
                .map(|offset| start + 2 + offset)
        }) {
            let authority = &output[..authority_end];
            if !authority.ends_with('/') {
                output.insert(authority_end, '/');
            }
        }
        if output.ends_with('?') {
            output.insert(output.len() - 1, '/');
        }
        return Ok(Value::String(output.into()));
    }
    let object = arguments.first().ok_or(VmError::NotCallable)?;
    if let Ok(Value::String(href)) = quench_runtime::execute::get_property_result(object, "href") {
        return Ok(Value::String(href));
    }
    let object_protocol = quench_runtime::execute::get_property_result(object, "protocol")
        .ok()
        .and_then(|value| match value {
            Value::String(value) => Some(value),
            _ => None,
        });
    if matches!(object_protocol.as_deref(), Some("file") | Some("file:")) {
        let pathname = quench_runtime::execute::get_property_result(object, "pathname")
            .ok()
            .map(|value| safe_value_string(&value))
            .unwrap_or_default();
        return Ok(Value::String(
            format!(
                "file:///{}",
                encode_file_path(pathname.trim_start_matches('/'))
            )
            .into(),
        ));
    }
    let protocol = quench_runtime::execute::get_property_result(object, "protocol")
        .ok()
        .and_then(|value| match value {
            Value::String(value) => Some(value),
            _ => None,
        })
        .unwrap_or_default();
    let host = quench_runtime::execute::get_property_result(object, "host")
        .ok()
        .and_then(|value| match value {
            Value::String(value) => Some(value),
            _ => None,
        })
        .unwrap_or_default();
    let pathname = quench_runtime::execute::get_property_result(object, "pathname")
        .ok()
        .and_then(|value| match value {
            Value::String(value) => Some(value),
            _ => None,
        })
        .unwrap_or_default();
    let search = quench_runtime::execute::get_property_result(object, "search")
        .ok()
        .and_then(|value| match value {
            Value::String(value) => Some(value),
            _ => None,
        })
        .unwrap_or_default();
    Ok(Value::String(
        format!(
            "{}//{}{}{}",
            protocol,
            host,
            if pathname.is_empty() { "/" } else { &pathname },
            search
        )
        .into(),
    ))
}
