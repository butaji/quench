//! Environment flags for the test262 runner.

#[derive(Debug, Clone)]
pub struct RunnerFlags {
    pub all_stages: bool,
    pub digest: bool,
    pub quick: bool,
    pub json_out: bool,
    pub isolated: bool,
    pub parallel: bool,
    pub stage: usize,
    pub quick_limit: usize,
    /// Re-run only paths listed in a prior `tasks/failures-N.json` digest.
    pub failed_json: Option<String>,
}

impl RunnerFlags {
    pub fn from_env() -> Self {
        Self {
            all_stages: env_bool("ALL_STAGES"),
            digest: env_bool("TEST262_DIGEST"),
            quick: env_bool("TEST262_QUICK"),
            json_out: env_bool("TEST262_JSON"),
            isolated: env_bool("TEST262_ISOLATED"),
            parallel: !env_bool("TEST262_SERIAL") && env_bool_default("TEST262_PARALLEL", true),
            stage: std::env::var("TEST262_STAGE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            quick_limit: std::env::var("TEST262_QUICK_LIMIT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(20),
            failed_json: std::env::var("TEST262_FAILED_JSON").ok(),
        }
    }
}

fn env_bool(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn env_bool_default(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(s) => s == "1" || s.eq_ignore_ascii_case("true"),
        Err(_) => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_bool_parses_true_values() {
        assert!(!env_bool("TEST262_FLAGS_UNSET_XYZ"));
    }

    #[test]
    fn default_parallel_is_on() {
        let f = RunnerFlags {
            all_stages: false,
            digest: true,
            quick: false,
            json_out: false,
            isolated: false,
            parallel: env_bool_default("TEST262_PARALLEL_UNSET_ABC", true),
            stage: 0,
            quick_limit: 20,
            failed_json: None,
        };
        assert!(f.parallel);
    }
}
