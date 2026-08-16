fn path_module() -> Value {
    let basename = capability_function(HostCapabilityKind::Custom(CapabilityName::PathBasename));
    let parse = capability_function(HostCapabilityKind::Custom(CapabilityName::PathParse));
    let format = capability_function(HostCapabilityKind::Custom(CapabilityName::PathFormat));
    let relative = capability_function(HostCapabilityKind::Custom(CapabilityName::PathRelative));
    let dirname = capability_function(HostCapabilityKind::Custom(CapabilityName::PathDirname));
    let absolute = capability_function(HostCapabilityKind::Custom(CapabilityName::PathIsAbsolute));
    let mut path = Value::object(vec![
        ("sep".into(), Value::String("/".into())),
        ("delimiter".into(), Value::String(":".into())),
        (
            "join".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::PathJoin)),
        ),
        (
            "extname".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::PathExtname)),
        ),
        (
            "normalize".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::PathNormalize)),
        ),
        ("basename".into(), basename.clone()),
        ("parse".into(), parse.clone()),
        ("format".into(), format.clone()),
        ("relative".into(), relative.clone()),
        ("dirname".into(), dirname.clone()),
        ("isAbsolute".into(), absolute.clone()),
        (
            "resolve".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::PathResolve)),
        ),
        (
            "matchesGlob".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::PathMatchesGlob)),
        ),
        (
            "toNamespacedPath".into(),
            capability_function(HostCapabilityKind::Custom(CapabilityName::PathToNamespaced)),
        ),
        (
            "posix".into(),
            Value::object(vec![
                ("sep".into(), Value::String("/".into())),
                ("delimiter".into(), Value::String(":".into())),
                (
                    "normalize".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathNormalize)),
                ),
                (
                    "extname".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathExtname)),
                ),
                ("basename".into(), basename),
                (
                    "join".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathJoin)),
                ),
                ("parse".into(), parse),
                ("format".into(), format),
                ("relative".into(), relative.clone()),
                ("dirname".into(), dirname.clone()),
                ("isAbsolute".into(), absolute.clone()),
                (
                    "resolve".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathResolve)),
                ),
                (
                    "matchesGlob".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::PathMatchesGlob,
                    )),
                ),
                (
                    "toNamespacedPath".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::PathToNamespaced,
                    )),
                ),
            ]),
        ),
        (
            "win32".into(),
            Value::object(vec![
                ("sep".into(), Value::String("\\".into())),
                ("delimiter".into(), Value::String(";".into())),
                (
                    "basename".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::PathWinBasename,
                    )),
                ),
                (
                    "extname".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathExtname)),
                ),
                (
                    "normalize".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::PathWinNormalize,
                    )),
                ),
                (
                    "parse".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathWinParse)),
                ),
                (
                    "format".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathWinFormat)),
                ),
                ("relative".into(), relative),
                ("dirname".into(), dirname),
                (
                    "isAbsolute".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::PathWinIsAbsolute,
                    )),
                ),
                (
                    "resolve".into(),
                    capability_function(HostCapabilityKind::Custom(CapabilityName::PathWinResolve)),
                ),
                (
                    "matchesGlob".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::PathWinMatchesGlob,
                    )),
                ),
                (
                    "toNamespacedPath".into(),
                    capability_function(HostCapabilityKind::Custom(
                        CapabilityName::PathWinToNamespaced,
                    )),
                ),
            ]),
        ),
    ]);
    path = quench_runtime::execute::set_property(path.clone(), "posix", path);
    NODE_PATH_MODULE.with(|module| module.replace(Some(path.clone())));
    path
}
