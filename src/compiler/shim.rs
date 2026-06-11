//! Quench Shim — React/Ink imports to ink.* global mappings
//!
//! This module defines how React and Ink imports are transformed into
//! Quench-compatible global references.
//!
//! ## Import Mappings
//!
//! | Source | Target |
//! |--------|--------|
//! | `import { useState } from "react"` | `const { useState } = ink;` |
//! | `import { render, Box } from "ink"` | `const { render, Box } = ink;` |
//! | `React.useState()` | `ink.useState()` |
//! | `React.createElement()` | `ink.createElement()` |

use std::collections::HashMap;

/// All supported React hooks
pub const REACT_HOOKS: &[&str] = &[
    "useState",
    "useEffect",
    "useRef",
    "useMemo",
    "useCallback",
    "useContext",
    "useReducer",
    "useLayoutEffect",
    "useImperativeHandle",
    "useDebugValue",
];

/// All supported Ink components and functions
/// Note: useState, useEffect, etc. are React hooks that Ink also supports
pub const INK_IMPORTS: &[&str] = &[
    // Components
    "Box",
    "Text",
    "Static",
    "Newline",
    "Spacer",
    "Fragment",
    // Functions
    "render",
    "createElement",
    // Hooks (Ink also supports React hooks)
    "useInput",
    "useApp",
    "useStdin",
    "useStdout",
    "useStderr",
    "useFocus",
    "useFocusManager",
    "measureElement",
    "createContext",
    // React hooks also work with Ink
    "useState",
    "useEffect",
    "useRef",
    "useMemo",
    "useCallback",
    "useContext",
    // Special
    "useBridge",
];

/// Check if a name is a React hook
pub fn is_react_hook(name: &str) -> bool {
    REACT_HOOKS.contains(&name)
}

/// Check if a name is an Ink import
pub fn is_ink_import(name: &str) -> bool {
    INK_IMPORTS.contains(&name)
}

/// Get all supported imports
pub fn get_all_imports() -> Vec<&'static str> {
    let mut all = REACT_HOOKS.to_vec();
    all.extend(INK_IMPORTS.iter().filter(|i| !REACT_HOOKS.contains(i)));
    all
}

/// Mapping of import names to their Quench global references
#[derive(Debug, Clone, Default)]
pub struct ImportShim {
    /// Maps local name -> ink.* reference
    pub mappings: HashMap<String, String>,
    /// Whether this import is from "react"
    pub is_react: bool,
    /// Whether this import is from "ink"
    pub is_ink: bool,
}

impl ImportShim {
    /// Create a new empty shim
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a React hook mapping
    pub fn add_react_hook(&mut self, name: &str) {
        self.is_react = true;
        if is_react_hook(name) {
            self.mappings.insert(name.to_string(), format!("ink.{}", name));
        }
    }

    /// Add an Ink component/function mapping
    pub fn add_ink_import(&mut self, name: &str) {
        self.is_ink = true;
        if is_ink_import(name) {
            self.mappings.insert(name.to_string(), format!("ink.{}", name));
        }
    }

    /// Check if this shim has any React imports
    pub fn has_react(&self) -> bool {
        self.is_react
    }

    /// Check if this shim has any Ink imports
    pub fn has_ink(&self) -> bool {
        self.is_ink
    }
}

/// Process polyfills that need special handling
#[derive(Debug, Clone)]
pub enum Polyfill {
    /// Replace process.exit with ink.useApp().exit()
    ProcessExit,
    /// Replace process.stdout.write with ink.stdout_write
    ProcessStdoutWrite,
    /// Replace process.stderr.write with ink.stderr_write
    ProcessStderrWrite,
    /// Replace process.env.NODE_ENV
    ProcessEnvNodeEnv,
    /// Unknown process.* access
    Unknown(String),
}

impl Polyfill {
    /// Check if an expression is a process polyfill
    pub fn from_member(member: &str) -> Option<Self> {
        match member {
            "exit" => Some(Polyfill::ProcessExit),
            "stdout" => Some(Polyfill::ProcessStdoutWrite),
            "stderr" => Some(Polyfill::ProcessStderrWrite),
            "env" => Some(Polyfill::ProcessEnvNodeEnv),
            other => Some(Polyfill::Unknown(other.to_string())),
        }
    }

    /// Generate the replacement code
    pub fn replacement(&self) -> Option<String> {
        match self {
            Polyfill::ProcessExit => Some("ink.useApp().exit".to_string()),
            Polyfill::ProcessStdoutWrite => Some("ink.stdout_write".to_string()),
            Polyfill::ProcessStderrWrite => Some("ink.stderr_write".to_string()),
            Polyfill::ProcessEnvNodeEnv => Some("\"production\"".to_string()),
            Polyfill::Unknown(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_react_hook() {
        assert!(is_react_hook("useState"));
        assert!(is_react_hook("useEffect"));
        assert!(!is_react_hook("render"));
    }

    #[test]
    fn test_is_ink_import() {
        assert!(is_ink_import("Box"));
        assert!(is_ink_import("Text"));
        assert!(is_ink_import("useState")); // useState is both React and Ink
        assert!(is_ink_import("useInput")); // Ink-specific
        assert!(!is_ink_import("unknown"));
    }

    #[test]
    fn test_import_shim() {
        let mut shim = ImportShim::new();
        shim.add_react_hook("useState");
        shim.add_ink_import("Box");

        assert!(shim.has_react());
        assert!(shim.has_ink());
        assert_eq!(shim.mappings.get("useState"), Some(&"ink.useState".to_string()));
        assert_eq!(shim.mappings.get("Box"), Some(&"ink.Box".to_string()));
    }

    #[test]
    fn test_polyfill_replacements() {
        assert_eq!(
            Polyfill::ProcessExit.replacement(),
            Some("ink.useApp().exit".to_string())
        );
        assert_eq!(
            Polyfill::ProcessEnvNodeEnv.replacement(),
            Some("\"production\"".to_string())
        );
    }
}
