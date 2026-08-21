fn url_object(url: &url::Url, id: u16) -> Result<Value, VmError> {
    let string = url.to_string();
    let password_cell = Rc::new(RefCell::new(Value::String(
        url.password().unwrap_or_default().into(),
    )));
    let pathname_cell = Rc::new(RefCell::new(Value::String(url.path().into())));
    let search_cell = Rc::new(RefCell::new(Value::String(
        url.query()
            .map(|query| format!("?{query}"))
            .unwrap_or_default()
            .into(),
    )));
    let search_params = Value::object(vec![
        (
            "get".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::UrlSearchParamsGet,
            )),
        ),
        (
            "sort".into(),
            capability_function(HostCapabilityKind::Custom(
                CapabilityName::UrlSearchParamsSort,
            )),
        ),
        (
            "getAll".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::UrlSearchParamsGetAll)),
        ),
        (
            "set".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::UrlSearchParamsSet)),
        ),
        (
            "toString".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::UrlSearchParamsToString)),
        ),
        ("\0urlId".into(), Value::Number(id as f64)),
    ]);
    let search_params = quench_runtime::execute::define_property(
        search_params,
        "__nodeURLOwner",
        quench_runtime::host_api::object(vec![
            (
                "get".into(),
                capability_function(HostCapabilityKind::Custom(
                    CapabilityName::UrlSearchParamsOwner,
                )),
            ),
            ("enumerable".into(), Value::Boolean(false)),
            ("configurable".into(), Value::Boolean(true)),
        ]),
    )?;
    let search_params = quench_runtime::execute::define_property(
        search_params,
        "\0urlId",
        quench_runtime::host_api::object(vec![
            ("value".into(), Value::Number(id as f64)),
            ("writable".into(), Value::Boolean(false)),
            ("enumerable".into(), Value::Boolean(false)),
            ("configurable".into(), Value::Boolean(true)),
        ]),
    )?;
    let object = quench_runtime::host_api::object(vec![
        ("href".into(), Value::String(string.clone())),
        ("searchParams".into(), search_params.clone()),
        (
            "origin".into(),
            Value::String(url.origin().ascii_serialization()),
        ),
        (
            "protocol".into(),
            Value::String(url.scheme().to_string() + ":"),
        ),
        (
            "username".into(),
            quench_runtime::host_api::object(vec![
                ("value".into(), Value::String(url.username().into())),
                ("writable".into(), Value::Boolean(true)),
                ("enumerable".into(), Value::Boolean(true)),
                ("configurable".into(), Value::Boolean(true)),
            ]),
        ),
        (
            "password".into(),
            Value::String(url.password().unwrap_or("").into()),
        ),
        (
            "hostname".into(),
            Value::String(url.host_str().unwrap_or("").into()),
        ),
        (
            "host".into(),
            Value::String(
                (url.host_str().unwrap_or("").to_string()
                    + &url
                        .port()
                        .map(|port| format!(":{port}"))
                        .unwrap_or_default())
                    .into(),
            ),
        ),
        (
            "port".into(),
            Value::String(url.port().map(|port| port.to_string()).unwrap_or_default()),
        ),
        ("pathname".into(), Value::String(url.path().into())),
        (
            "hostname".into(),
            Value::String(url.host_str().unwrap_or("").into()),
        ),
        (
            "path".into(),
            Value::String(
                format!(
                    "{}{}",
                    url.path(),
                    url.query()
                        .map(|query| format!("?{query}"))
                        .unwrap_or_default()
                )
                .into(),
            ),
        ),
        (
            "query".into(),
            Value::String(url.query().unwrap_or("").into()),
        ),
        ("search".into(), Value::BindingCell(search_cell.clone())),
        (
            "hash".into(),
            Value::String(
                url.fragment()
                    .map(|fragment| format!("#{fragment}"))
                    .unwrap_or_default(),
            ),
        ),
        (
            "toString".into(),
            capability_function(HostCapabilityKind::Custom(id)),
        ),
        (
            "toJSON".into(),
            capability_function(HostCapabilityKind::Custom(id)),
        ),
        ("\0urlBrand".into(), Value::Boolean(true)),
        ("\0passwordValue".into(), Value::BindingCell(password_cell)),
        ("\0pathnameValue".into(), Value::BindingCell(pathname_cell)),
        (
            "\0searchValue".into(),
            Value::BindingCell(search_cell.clone()),
        ),
        ("\0hrefValue".into(), Value::String(string.clone())),
    ]);
    let object = object;
    let object = quench_runtime::execute::define_property(
        object,
        "password",
        quench_runtime::host_api::object(vec![
            (
                "get".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::UrlPasswordGet)),
            ),
            (
                "set".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::UrlPasswordSet)),
            ),
            ("enumerable".into(), Value::Boolean(true)),
            ("configurable".into(), Value::Boolean(true)),
        ]),
    )?;
    let object = quench_runtime::execute::define_property(
        object,
        "pathname",
        quench_runtime::host_api::object(vec![
            (
                "get".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::UrlPathnameGet)),
            ),
            (
                "set".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::UrlPathnameSet)),
            ),
            ("enumerable".into(), Value::Boolean(true)),
            ("configurable".into(), Value::Boolean(true)),
        ]),
    )?;
    let object = quench_runtime::execute::define_property(
        object,
        "search",
        quench_runtime::host_api::object(vec![
            (
                "get".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::UrlSearchGet)),
            ),
            (
                "set".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::UrlSearchSet)),
            ),
            ("enumerable".into(), Value::Boolean(true)),
            ("configurable".into(), Value::Boolean(true)),
        ]),
    )?;
    let object = object;
    let object = quench_runtime::execute::define_property(
        object,
        "searchParams",
        quench_runtime::host_api::object(vec![
            ("value".into(), search_params),
            ("writable".into(), Value::Boolean(false)),
            ("enumerable".into(), Value::Boolean(false)),
            ("configurable".into(), Value::Boolean(true)),
        ]),
    )?;
    let object = quench_runtime::execute::define_property(
        object,
        "href",
        quench_runtime::host_api::object(vec![
            (
                "get".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::UrlHrefGet)),
            ),
            (
                "set".into(),
                capability_function(HostCapabilityKind::Custom(CapabilityName::UrlHrefSet)),
            ),
            ("enumerable".into(), Value::Boolean(true)),
            ("configurable".into(), Value::Boolean(true)),
        ]),
    )?;
    let mut object = object;
    for name in [
        "origin", "hostname", "host", "port", "path", "query", "toString", "toJSON",
    ] {
        let value = quench_runtime::execute::get_property_result(&object, name)?;
        object = quench_runtime::execute::define_property(
            object,
            name,
            quench_runtime::host_api::object(vec![
                ("value".into(), value),
                ("writable".into(), Value::Boolean(true)),
                ("enumerable".into(), Value::Boolean(false)),
                ("configurable".into(), Value::Boolean(true)),
            ]),
        )?;
    }
    for (name, getter, setter) in [
        (
            "href",
            CapabilityName::UrlHrefGet,
            Some(CapabilityName::UrlHrefSet),
        ),
        (
            "password",
            CapabilityName::UrlPasswordGet,
            Some(CapabilityName::UrlPasswordSet),
        ),
        (
            "pathname",
            CapabilityName::UrlPathnameGet,
            Some(CapabilityName::UrlPathnameSet),
        ),
        (
            "search",
            CapabilityName::UrlSearchGet,
            Some(CapabilityName::UrlSearchSet),
        ),
    ] {
        object = quench_runtime::execute::define_property(
            object,
            name,
            quench_runtime::host_api::object(vec![
                (
                    "get".into(),
                    capability_function(HostCapabilityKind::Custom(getter)),
                ),
                (
                    "set".into(),
                    capability_function(HostCapabilityKind::Custom(setter.unwrap())),
                ),
                ("enumerable".into(), Value::Boolean(false)),
                ("configurable".into(), Value::Boolean(true)),
            ]),
        )?;
    }
    Ok(object)
}
