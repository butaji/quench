//! Invocation metadata owned by the isolated fixture worker.

#[derive(Debug, Default)]
pub(crate) struct FixtureMetadata {
    pub(crate) flags: Vec<String>,
    pub(crate) env: Vec<(String, String)>,
}

/// Parse Node's fixture directives once before launching the worker.
pub(crate) fn fixture_metadata(source: &str) -> FixtureMetadata {
    let (flags, env) = source
        .lines()
        .filter_map(|line| {
            let comment = line.trim_start().strip_prefix("//")?;
            let directive = comment.trim_start();
            if comment.len() == directive.len() {
                return None;
            }
            directive
                .strip_prefix("Flags:")
                .map(|value| (Some(value), None))
                .or_else(|| {
                    directive
                        .strip_prefix("Env:")
                        .map(|value| (None, Some(value)))
                })
        })
        .fold(
            (Vec::new(), Vec::new()),
            |(mut flags, mut env), (found_flags, found_env)| {
                if let Some(values) = found_flags {
                    flags.extend(values.split_whitespace().map(str::to_owned));
                }
                if let Some(values) = found_env {
                    env.extend(values.split_whitespace().filter_map(|entry| {
                        let (name, value) = entry.split_once('=')?;
                        (!name.is_empty()).then(|| (name.to_owned(), value.to_owned()))
                    }));
                }
                (flags, env)
            },
        );
    FixtureMetadata { flags, env }
}

pub(crate) fn fixture_flags(source: &str) -> Vec<String> {
    fixture_metadata(source).flags
}
