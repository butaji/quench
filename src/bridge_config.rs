//! Bridge Configuration — Propagate props from Rust to Ink/JS runtime
//!
//! Provides `BridgeConfig` to inject CLI flags, platform info, and terminal
//! capabilities into the QuickJS VM before user code runs.
//!
//! **IMPORTANT:** Keep `BRIDGE_GLOBAL` in sync with `runtime.js`.
//! See `src/runtime.js:BRIDGE_GLOBAL`.

use std::collections::HashMap;

/// The global name used to expose the bridge API to JavaScript.
/// Keep in sync with `runtime.js:BRIDGE_GLOBAL`.
pub const BRIDGE_GLOBAL: &str = "__quench";

/// Bridge configuration injected into the JS runtime
#[derive(Debug, Clone, Default)]
pub struct BridgeConfig {
    /// User-defined `--prop KEY=VALUE` pairs
    pub props: HashMap<String, String>,
    /// Platform detection (OS, arch, version)
    pub platform: PlatformInfo,
    /// Terminal capability detection
    pub terminal: TerminalInfo,
}

/// Platform information
#[derive(Debug, Clone)]
pub struct PlatformInfo {
    pub os: String,
    pub arch: String,
    pub version: String,
}

impl Default for PlatformInfo {
    fn default() -> Self {
        Self {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Terminal capability information
#[derive(Debug, Clone)]
pub struct TerminalInfo {
    /// 0 = none, 8 = 256, 16 = high, 24 = truecolor
    pub color_support: u8,
    pub has_mouse: bool,
    pub has_unicode: bool,
}

impl Default for TerminalInfo {
    fn default() -> Self {
        Self {
            color_support: detect_color_support(),
            has_mouse: true,
            has_unicode: true,
        }
    }
}

impl BridgeConfig {
    /// Create a new BridgeConfig with auto-detected platform/terminal info
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a user-defined prop (fluent API)
    pub fn with_prop(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.props.insert(key.into(), value.into());
        self
    }

    /// Parse `--prop KEY=VALUE` flags from CLI arguments
    pub fn from_args(args: &[String]) -> Self {
        let mut config = Self::new();
        let mut i = 0;
        while i < args.len() {
            if args[i] == "--prop" || args[i] == "-p" {
                if let Some(pair) = args.get(i + 1) {
                    if let Some((key, value)) = pair.split_once('=') {
                        config.props.insert(key.to_string(), value.to_string());
                    }
                    i += 2;
                    continue;
                }
            }
            i += 1;
        }
        config
    }

    /// Generate a JS snippet that injects `globalThis.__quench.config`
    /// Uses `BRIDGE_GLOBAL` constant to ensure sync with runtime.js.
    pub fn to_js_injection(&self) -> String {
        let props_json = if self.props.is_empty() {
            String::new()
        } else {
            let mut parts = Vec::new();
            for (k, v) in &self.props {
                parts.push(format!(
                    "{}:{}",
                    serde_json::to_string(k).unwrap_or_default(),
                    serde_json::to_string(v).unwrap_or_default()
                ));
            }
            parts.join(",")
        };

        let platform_json = format!(
            r#"platform: {{
      os: {},
      arch: {},
      version: {}
    }}"#,
            serde_json::to_string(&self.platform.os).unwrap_or_default(),
            serde_json::to_string(&self.platform.arch).unwrap_or_default(),
            serde_json::to_string(&self.platform.version).unwrap_or_default(),
        );

        let terminal_json = format!(
            r#"terminal: {{
      colorSupport: {},
      hasMouse: {},
      hasUnicode: {}
    }}"#,
            self.terminal.color_support,
            self.terminal.has_mouse,
            self.terminal.has_unicode,
        );

        // Use BRIDGE_GLOBAL constant - keep in sync with runtime.js
        if props_json.is_empty() {
            format!(
                r#"globalThis.{BRIDGE_GLOBAL} = {{ config: {{ {}, {} }} }};"#,
                platform_json,
                terminal_json
            )
        } else {
            format!(
                r#"globalThis.{BRIDGE_GLOBAL} = {{ config: {{ {}, {}, {} }} }};"#,
                props_json,
                platform_json,
                terminal_json
            )
        }
    }
}

/// Detect terminal color support using environment heuristics
fn detect_color_support() -> u8 {
    if std::env::var("COLORTERM").map(|s| s == "truecolor" || s == "24bit").unwrap_or(false) {
        return 24;
    }
    if std::env::var("TERM").map(|s| s.contains("256color")).unwrap_or(false) {
        return 8;
    }
    if std::env::var("TERM").map(|s| s.contains("color")).unwrap_or(false) {
        return 8;
    }
    if atty::is(atty::Stream::Stdout) {
        // Modern terminals almost all support truecolor
        return 24;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bridge_config_new() {
        let config = BridgeConfig::new();
        assert!(!config.platform.os.is_empty());
        assert!(!config.platform.arch.is_empty());
        assert!(config.terminal.has_mouse);
    }

    #[test]
    fn test_bridge_config_from_args() {
        let args = vec![
            "quench".to_string(),
            "--prop".to_string(),
            "theme=dark".to_string(),
            "--prop".to_string(),
            "locale=en-US".to_string(),
            "examples/app.js".to_string(),
        ];
        let config = BridgeConfig::from_args(&args);
        assert_eq!(config.props.get("theme"), Some(&"dark".to_string()));
        assert_eq!(config.props.get("locale"), Some(&"en-US".to_string()));
    }

    #[test]
    fn test_bridge_config_js_injection() {
        let config = BridgeConfig::new()
            .with_prop("theme", "dark")
            .with_prop("locale", "en-US");
        let js = config.to_js_injection();
        assert!(js.contains(&format!("globalThis.{BRIDGE_GLOBAL}")));
        assert!(js.contains("theme"));
        assert!(js.contains("dark"));
        assert!(js.contains("platform"));
        assert!(js.contains("terminal"));
        assert!(js.contains("colorSupport"));
    }

    #[test]
    fn test_empty_props() {
        let config = BridgeConfig::new();
        let js = config.to_js_injection();
        assert!(js.contains(&format!("globalThis.{BRIDGE_GLOBAL}")));
        assert!(js.contains("platform"));
        // No trailing comma issues
        assert!(!js.contains(",\n    ,"));
    }

    #[test]
    fn test_detect_color_support() {
        // Just verify it returns a valid value without panicking
        let support = detect_color_support();
        assert!(support == 0 || support == 8 || support == 16 || support == 24);
    }
}
