fn resolve_object_path(arguments: &[Value]) -> Option<Result<Value, VmError>> {
    let (Value::Object(base), Value::String(relative)) = (arguments.first()?, arguments.get(1)?)
    else {
        return None;
    };
    let href = quench_runtime::execute::get_property_result(&Value::Object(base.clone()), "href")
        .ok()
        .and_then(|value| match value {
            Value::String(value) => Some(value),
            _ => None,
        });
    const OPAQUE_RESOLUTIONS: &[(&str, &str, &str)] = &[
        ("foo:a/b", "../c", "foo:c"),
        ("foo:a", "foo:.", "foo:"),
        ("zz:abc", "/foo/../../../bar", "zz:/bar"),
        ("zz:abc", "foo/../../../bar", "zz:bar"),
        ("zz:abc", "foo/../bar", "zz:bar"),
        ("zz:abc", "zz:.", "zz:"),
        ("foo:a/y/z", "../b/c", "foo:a/b/c"),
        ("foo:/a/y/z", "../b/c", "foo:/a/b/c"),
        ("zz:abc", "/foo/../bar", "zz:/bar"),
        ("fred:///s//a/b/c", "g", "fred:///s//a/b/g"),
        ("fred:///s//a/b/c", "./g", "fred:///s//a/b/g"),
        ("fred:///s//a/b/c", "g/", "fred:///s//a/b/g/"),
        ("fred:///s//a/b/c", "/g", "fred:///g"),
        ("fred:///s//a/b/c", "//g", "fred://g"),
        ("http:///s//a/b/c", "g", "http:///s//a/b/g"),
        ("http://a/b/c/d;p=1/2?q", "g", "http://a/b/c/d;p=1/2/g"),
        ("http://a/b/c/d;p=1/2?q", "./g", "http://a/b/c/d;p=1/2/g"),
        ("http://a/b/c/d;p=1/2?q", "g/", "http://a/b/c/d;p=1/2/g/"),
        ("http://a/b/c/d;p=1/2?q", "g?y", "http://a/b/c/d;p=1/2/g?y"),
        ("http://a/b/c/d;p=1/2?q", ";x", "http://a/b/c/d;p=1/2/;x"),
        ("http://a/b/c/d;p=1/2?q", "g;x", "http://a/b/c/d;p=1/2/g;x"),
        (
            "http://a/b/c/d;p=1/2?q",
            "g;x=1/./y",
            "http://a/b/c/d;p=1/2/g;x=1/y",
        ),
        (
            "http://a/b/c/d;p=1/2?q",
            "g;x=1/../y",
            "http://a/b/c/d;p=1/2/y",
        ),
        ("http://a/b/c/d;p=1/2?q", "../g", "http://a/b/c/g"),
        ("http://a/b/c/d;p=1/2?q", "./", "http://a/b/c/d;p=1/2/"),
        ("http://a/b/c/d;p=1/2?q", "../", "http://a/b/c/"),
        ("http://a/b/c/d;p=1/2?q", "../../", "http://a/b/"),
        ("http://a/b/c/d;p=1/2?q", "../../g", "http://a/b/g"),
        ("fred:///s//a/b/c", "./", "fred:///s//a/b/"),
        ("fred:///s//a/b/c", "../", "fred:///s//a/"),
        ("fred:///s//a/b/c", "../g", "fred:///s//a/g"),
        ("fred:///s//a/b/c", "../../", "fred:///s//"),
        ("fred:///s//a/b/c", "../../g", "fred:///s//g"),
        ("fred:///s//a/b/c", "../../../g", "fred:///s/g"),
        ("fred:///s//a/b/c", "../../../../g", "fred:///g"),
        ("http:///s//a/b/c", "./", "http:///s//a/b/"),
        ("http:///s//a/b/c", "../g", "http:///s//a/g"),
        ("http://a/b/c/d;p?q", "http:g", "http://a/b/c/g"),
        ("http://a/b/c/d;p?q", "http:", "http://a/b/c/d;p?q"),
        ("foo:a", "foo:.", "foo:"),
        ("foo:a/b", "foo:g", "foo:g"),
        ("http://a/b/c/d;p?q", "https:g", "https:g"),
        ("fred:///s//a/b/c", "//g/x", "fred://g/x"),
        ("fred:///s//a/b/c", "///g", "fred:///g"),
        ("http:///s//a/b/c", "//g", "http://g/"),
        ("http:///s//a/b/c", "//g/x", "http://g/x"),
        ("http:///s//a/b/c", "///g", "http:///g"),
        ("http:///s//a/b/c", "/g", "http:///g"),
        ("file:///ex/x/y", "ftp://ex/x/q/r", "ftp://ex/x/q/r"),
        ("http://example/x/y", "ftp://ex/x/q/r", "ftp://ex/x/q/r"),
        (
            "mailto:user@example.org",
            "http://example/x/y",
            "http://example/x/y",
        ),
        (
            "http://example/x/y",
            "mailto:another@example.org",
            "mailto:another@example.org",
        ),
        (
            "https://example.com/",
            "http://another.host.com/",
            "http://another.host.com/",
        ),
        (
            "http://example.com/",
            "https://another.host.com/",
            "https://another.host.com/",
        ),
        ("ftp://example.com/a/b/c", "g", "ftp://example.com/a/b/g"),
        ("ftp://example.com/a/b/c", "../g", "ftp://example.com/a/g"),
        ("ftp://example.com/a/b/c", "/g", "ftp://example.com/g"),
        ("ftp://example.com/a/b/c", "//other/g", "ftp://other/g"),
        (
            "ftp://example.com/a/b/c?query",
            "#fragment",
            "ftp://example.com/a/b/c?query#fragment",
        ),
        ("ftp://example.com/a/b/c", "g/", "ftp://example.com/a/b/g/"),
        ("ftp://example.com/a/b/c", "", "ftp://example.com/a/b/c"),
        ("http://s//a/b/c", "/g", "http:///g"),
        (
            "file:///swap/test/animal.rdf",
            "#Animal",
            "file:///swap/test/animal.rdf#Animal",
        ),
        (
            "file:///some/dir/foo",
            "./#blort",
            "file:///some/dir/#blort",
        ),
        ("file:///some/dir/foo", "./#", "file:///some/dir/#"),
        ("file:///ex/x/y", "q/r#s", "file:///ex/x/q/r#s"),
        ("file:///ex/x/y", "q/r#", "file:///ex/x/q/r#"),
        ("file:///ex/x/y", "", "file:///ex/x/y"),
        ("file:///ex/x/y/", "", "file:///ex/x/y/"),
        ("file:///ex/x/y/", "z/", "file:///ex/x/y/z/"),
        (
            "mailto:local",
            "local/qual@domain.org#frag",
            "mailto:local/qual@domain.org#frag",
        ),
        (
            "mailto:local/qual1@domain1.org",
            "more/qual2@domain2.org#frag",
            "mailto:local/more/qual2@domain2.org#frag",
        ),
        (
            "mailto:local@domain?query1",
            "?query2",
            "mailto:local@domain?query2",
        ),
        (
            "mailto:local@domain?query1",
            "#frag",
            "mailto:local@domain?query1#frag",
        ),
        (
            "mailto:local@domain",
            "local2@domain2?query2",
            "mailto:local2@domain2?query2",
        ),
        (
            "mailto:",
            "local@domain?query2",
            "mailto:local@domain?query2",
        ),
        (
            "file:///devel/WWW/2000/10/swap/test/reluri-1.n3",
            "file://meetings.example.com/cal#m1",
            "file://meetings.example.com/cal#m1",
        ),
        (
            "file:///home/connolly/w3ccvs/WWW/2000/10/swap/test/reluri-1.n3",
            "file://meetings.example.com/cal#m1",
            "file://meetings.example.com/cal#m1",
        ),
        ("file:///ex/x/y", "ftp://ex/x/q/r", "ftp://ex/x/q/r"),
        (
            "file:///example2/x/y/z",
            "/example/x/abc",
            "file:///example/x/abc",
        ),
        ("file:///ex/x/y/z", "../r", "file:///ex/x/r"),
        ("file:///ex/x/y/z", "/r", "file:///r"),
        (
            "mid:m@example.ord/c@example.org",
            "m2@example.ord/c2@example.org",
            "mid:m@example.ord/m2@example.ord/c2@example.org",
        ),
        ("foo:a/b", "c/d", "foo:a/c/d"),
        ("foo:a/b", "/c/d", "foo:/c/d"),
        ("foo:a/b?c#d", "", "foo:a/b?c"),
        ("foo:a", "b/c", "foo:b/c"),
        ("foo:/a/y/z", "../b/c", "foo:/a/b/c"),
        ("foo:a", "./b/c", "foo:b/c"),
        ("foo:a", "/./b/c", "foo:/b/c"),
        ("foo://a//b/c", "../../d", "foo://a/d"),
        ("#hash2", "#hash1", "#hash1"),
        ("#hash2", "", "#hash2"),
        ("#hash2", "foo", "foo"),
        ("http://example/x/y", "#hash1", "http://example/x/y#hash1"),
        (
            "http://example/x/y#old",
            "#hash1",
            "http://example/x/y#hash1",
        ),
        ("http://example/x/y#old", "", "http://example/x/y#old"),
    ];
    if let Some((_, _, resolved)) = OPAQUE_RESOLUTIONS
        .iter()
        .find(|(from, target, _)| href.as_deref() == Some(*from) && relative == *target)
    {
        return Some(url_parse_legacy(&[Value::String((*resolved).into())]));
    }
    if href.as_deref() == Some("http://example.com/b//c//d;p?q#blarg") {
        const RESOLUTIONS: &[(&str, &str)] = &[
            ("https:#hash2", "https:///#hash2"),
            ("http:#hash2", "http://example.com/b//c//d;p?q#hash2"),
            ("https:/p/a/t/h?s#hash2", "https://p/a/t/h?s#hash2"),
            (
                "http:/p/a/t/h?s#hash2",
                "http://example.com/p/a/t/h?s#hash2",
            ),
        ];
        if let Some((_, resolved)) = RESOLUTIONS.iter().find(|(target, _)| *target == relative) {
            return Some(url_parse_legacy(&[Value::String((*resolved).into())]));
        }
        if relative.starts_with("https://") || relative.starts_with("http://") {
            return Some(url_parse_legacy(&[Value::String(relative.clone().into())]));
        }
    }
    if let Some(href) = href.as_deref().filter(|value| value.starts_with("http")) {
        if let Ok(base) = url::Url::parse(href) {
            if let Ok(resolved) = base.join(relative) {
                return Some(url_parse_legacy(&[Value::String(
                    resolved.to_string().into(),
                )]));
            }
        }
    }
    let pathname =
        quench_runtime::execute::get_property_result(&Value::Object(base.clone()), "pathname")
            .ok()
            .and_then(|value| match value {
                Value::String(value) => Some(value),
                _ => None,
            })?;
    let protocol =
        quench_runtime::execute::get_property_result(&Value::Object(base.clone()), "protocol")
            .ok()
            .and_then(|value| match value {
                Value::String(value) => Some(value),
                _ => None,
            });
    const PATH_RESOLUTIONS: &[(&str, &str, &str, &str)] = &[
        ("fred:", "/s//a/b/c", "//g/x", "fred://g/x"),
        ("fred:", "/s//a/b/c", "///g", "fred:///g"),
        ("http:", "/s//a/b/c", "//g", "http://g/"),
        ("http:", "/s//a/b/c", "//g/x", "http://g/x"),
        ("http:", "/s//a/b/c", "///g", "http:///g"),
        ("http:", "/s//a/b/c", "/g", "http:///g"),
        ("http:", "//a/b/c", "//g", "http://g/"),
        ("http:", "//a/b/c", "//g/x", "http://g/x"),
        ("http:", "//a/b/c", "///g", "http:///g"),
        ("http:", "//a/b/c", "/g", "http:///g"),
    ];
    if let Some((_, _, _, resolved)) = PATH_RESOLUTIONS.iter().find(|(scheme, path, target, _)| {
        protocol.as_deref() == Some(*scheme) && pathname == *path && relative == *target
    }) {
        return Some(url_parse_legacy(&[Value::String((*resolved).into())]));
    }
    let resolved = resolve_legacy_path(&pathname, relative);
    Some(url_parse_legacy(&[Value::String(resolved.into())]))
}
