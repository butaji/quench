//! Runtime-neutral WHATWG URL parsing and field projection.
//!
//! Runtime facades own input coercion and error construction. This module is
//! the parser and field authority for the shared WHATWG URL adapter.

/// A parsed WHATWG URL.
#[derive(Clone, Debug)]
pub enum Parsed {
    Url(url::Url),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ParseError {
    InvalidUrl,
}

impl Parsed {
    /// Strict WHATWG parsing used by the shared URL constructor.
    pub fn parse_strict(input: &str, base: Option<&str>) -> Result<Self, ParseError> {
        let parsed = match base {
            Some(base) => url::Url::parse(base).and_then(|base| base.join(input)),
            None => url::Url::parse(input),
        }
        .map_err(|_| ParseError::InvalidUrl)?;
        Ok(Self::Url(parsed))
    }

    pub fn href(&self) -> String {
        match self {
            Self::Url(url) => url.as_str().to_owned(),
        }
    }

    pub fn get(&self, component: &str) -> String {
        if component == "href" {
            return self.href();
        }
        let Self::Url(url) = self;
        match component {
            "protocol" => format!("{}:", url.scheme()),
            "username" => url.username().to_owned(),
            "password" => url.password().unwrap_or_default().to_owned(),
            "host" => host_string(url, true),
            "hostname" => url.host_str().unwrap_or_default().to_owned(),
            "port" => url.port().map(|port| port.to_string()).unwrap_or_default(),
            "pathname" => url.path().to_owned(),
            "search" => url
                .query()
                .map(|query| format!("?{query}"))
                .unwrap_or_default(),
            "hash" => url
                .fragment()
                .map(|fragment| format!("#{fragment}"))
                .unwrap_or_default(),
            "origin" => origin_string(url),
            _ => String::new(),
        }
    }
}

fn host_string(url: &url::Url, with_port: bool) -> String {
    let mut host = url.host_str().unwrap_or_default().to_owned();
    if with_port {
        if let Some(port) = url.port() {
            host.push(':');
            host.push_str(&port.to_string());
        }
    }
    host
}

fn origin_string(url: &url::Url) -> String {
    match url.origin() {
        url::Origin::Tuple(scheme, host, port) => {
            let default_port = match scheme.as_str() {
                "http" | "ws" => 80,
                "https" | "wss" => 443,
                "ftp" => 21,
                _ => 0,
            };
            if port == default_port {
                format!("{scheme}://{host}")
            } else {
                format!("{scheme}://{host}:{port}")
            }
        }
        url::Origin::Opaque(_) => "null".to_owned(),
    }
}
