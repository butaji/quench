use std::collections::BTreeMap;

const FIELD_ORDER: &[&str] = &[
    "protocol", "slashes", "auth", "host", "port", "hostname", "hash", "search",
    "query", "pathname", "path", "href",
];

pub(crate) fn parse_legacy_parts(input: &str) -> Result<LegacyUrlParts, LegacyUrlParseError> {
    let raw = input.trim_matches(|c: char| c <= '\u{20}');
    if raw.is_empty() { return Ok(parts(BTreeMap::new(), None, false)); }
    if let Some((_, authority)) = raw.split_once("://") {
        let authority = authority.split(['/', '?', '#']).next().unwrap_or(authority);
        if let Some((auth, _)) = authority.rsplit_once('@') {
            if malformed_percent(auth) { return Err(LegacyUrlParseError::MalformedUri); }
        }
    }
    let mut fields = BTreeMap::new();
    let mut query_source = None;
    let mut query_object = false;
    if raw.contains(':') || raw.starts_with("//") {
        let normalized = normalize_input(raw);
        let parsed = if normalized.starts_with("//") { format!("http:{normalized}") } else { normalized.clone() };
        let parsed_url = ::url::Url::parse(&parsed).map_err(|_| invalid("ERR_INVALID_URL", raw))?;
        let protocol = parsed_url.scheme().to_owned() + ":";
        fields.insert("protocol".into(), protocol.clone());
        let has_authority = raw.contains("://") || raw.starts_with("//");
        if has_authority { fields.insert("slashes".into(), "true".into()); }
        let username = parsed_url.username();
        let password = parsed_url.password();
        if !username.is_empty() || password.is_some() {
            let mut auth = username.to_owned();
            if let Some(password) = password { auth.push(':'); auth.push_str(password); }
            fields.insert("auth".into(), auth);
        }
        if let Some(hostname) = parsed_url.host_str() {
            let hostname = hostname.to_ascii_lowercase();
            let host = parsed_url.host().map(|host| host.to_string()).unwrap_or_else(|| hostname.clone());
            let host = parsed_url.port().map_or(host.clone(), |port| format!("{host}:{port}"));
            fields.insert("host".into(), host);
            fields.insert("hostname".into(), hostname);
        }
        if let Some(port) = parsed_url.port() { fields.insert("port".into(), port.to_string()); }
        let pathname = encode_path_component(parsed_url.path());
        if !pathname.is_empty() { fields.insert("pathname".into(), pathname.clone()); }
        if let Some(query) = parsed_url.query() {
            query_source = Some(query.to_owned());
            let search = format!("?{}", encode_query(query));
            fields.insert("search".into(), search.clone());
            fields.insert("query".into(), encode_query(query));
            query_object = true;
        }
        if let Some(hash) = parsed_url.fragment() { fields.insert("hash".into(), format!("#{}", encode_path_component(hash))); }
        let pathname = fields.get("pathname").map(String::as_str).unwrap_or_default();
        let search = fields.get("search").map(String::as_str).unwrap_or_default();
        if !pathname.is_empty() || !search.is_empty() { fields.insert("path".into(), format!("{pathname}{search}")); }
        let href = parsed_url.as_str().trim_end_matches('#').to_owned();
        fields.insert("href".into(), href);
    } else {
        query_object = true;
        let (without_hash, hash) = raw.split_once('#').map_or((raw, None), |(a, b)| (a, Some(b)));
        let (pathname, query) = without_hash.split_once('?').map_or((without_hash, None), |(a, b)| (a, Some(b)));
        let pathname = encode_path_component(pathname);
        if !pathname.is_empty() { fields.insert("pathname".into(), pathname.clone()); }
        if let Some(query) = query {
            query_source = Some(query.to_owned());
            let encoded = encode_query(query);
            fields.insert("query".into(), encoded.clone());
            fields.insert("search".into(), format!("?{encoded}"));
        }
        let path = format!("{}{}", pathname, fields.get("search").map(String::as_str).unwrap_or_default());
        if !path.is_empty() { fields.insert("path".into(), path.clone()); }
        if let Some(hash) = hash { fields.insert("hash".into(), format!("#{}", encode_path_component(hash))); }
        let href = format!("{}{}", path, fields.get("hash").map(String::as_str).unwrap_or_default());
        if !href.is_empty() { fields.insert("href".into(), href); }
    }
    Ok(parts(fields, query_source, query_object))
}

fn parts(mut fields: BTreeMap<String, String>, query: Option<String>, query_object: bool) -> LegacyUrlParts {
    let fields = FIELD_ORDER.iter().map(|key| {
        let value = match fields.remove(*key) {
            Some(value) if *key == "slashes" && value == "true" => LegacyUrlField::Boolean(true),
            Some(value) => LegacyUrlField::Text(value),
            None => LegacyUrlField::Null,
        };
        (*key, value)
    }).collect();
    LegacyUrlParts { fields, query, query_object }
}

fn invalid(code: &str, input: &str) -> LegacyUrlParseError {
    LegacyUrlParseError::Invalid { code: code.to_owned(), input: input.to_owned() }
}

fn malformed_percent(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' { decoded.push(bytes[index]); index += 1; continue; }
        if index + 2 >= bytes.len() { return true; }
        let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) else { return true; };
        decoded.push((high << 4) | low);
        index += 3;
    }
    std::str::from_utf8(&decoded).is_err()
}

fn hex(byte: u8) -> Option<u8> {
    match byte { b'0'..=b'9' => Some(byte - b'0'), b'a'..=b'f' => Some(byte - b'a' + 10), b'A'..=b'F' => Some(byte - b'A' + 10), _ => None }
}

fn normalize_input(value: &str) -> String {
    value.replace("http:\\\\\\\\", "http://").replace('\\', "/")
}

fn encode_query(value: &str) -> String {
    value.replace(' ', "%20").replace('"', "%22").replace('\'', "%27")
        .replace('^', "%5E").replace('`', "%60").replace('{', "%7B")
        .replace('}', "%7D").replace('|', "%7C").replace('\\', "%5C")
}

fn encode_path_component(path: &str) -> String {
    path.chars().map(|c| match c {
        '\t' => "%09".into(), '\n' => "%0A".into(), '\r' => "%0D".into(),
        ' ' => "%20".into(), '"' => "%22".into(), '<' => "%3C".into(),
        '>' => "%3E".into(), '`' => "%60".into(), '#' => "%23".into(),
        '?' => "%3F".into(), '\'' => "%27".into(), '{' => "%7B".into(),
        '}' => "%7D".into(), '|' => "%7C".into(), '\\' => "%5C".into(),
        '^' => "%5E".into(), _ => c.to_string(),
    }).collect()
}
