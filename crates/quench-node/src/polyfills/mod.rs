//! Polyfill registry: Node-compat abilities as data.
//!
//! Every polyfill is an [`Ability`]: a named unit with a lifecycle phase
//! and, today, a JS bridge source evaluated by the engine. Abilities live
//! one-per-file under `bootstrap/` and `post_bootstrap/` and are declared
//! exactly once via the `abilities!` table macro — the macro generates the
//! module declarations, the ordered `ABILITIES` table, and the name lookup.
//!
//! ## Adding abilities
//!
//! The runtime composes the full Node surface by chained `add` calls on
//! [`Registry`] ("add all the necessary abilities"):
//!
//! ```ignore
//! let registry = Registry::new()
//!     .add_all(bootstrap::ABILITIES)
//!     .add_all(post_bootstrap::ABILITIES);
//! ```
//!
//! ## Future work: native dispatch
//!
//! Today every ability installs through `js` (a Rust string → engine eval).
//! To bypass the JS layer for a hot ability — or to retarget the registry
//! at another engine with direct Rust API calls — implement
//! [`DirectCallable`] for it; the runtime tries native dispatch first and
//! falls back to evaluating `js`.

/// Lifecycle phase in which an ability installs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Concatenated into the single bootstrap source evaluated first.
    Bootstrap,
    /// Evaluated fragment-by-fragment after the bootstrap source.
    PostBootstrap,
}

/// One installable ability: a named polyfill fragment.
#[derive(Clone, Copy)]
pub struct Ability {
    /// Stable kebab-case name (e.g. `"buffer-validation"`).
    pub name: &'static str,
    /// Lifecycle phase.
    pub phase: Phase,
    /// JS bridge source evaluated by the engine. The seam at which a
    /// native Rust implementation can later take over.
    pub js: &'static str,
}

/// Forward-compatibility hook for hot-path abilities.
///
/// Returning `Some` from `try_call` means the call was handled natively;
/// `None` falls back to the ability's JS bridge.
#[allow(dead_code)]
pub trait DirectCallable {
    fn try_call(_args: DirectCallArgs<'_>) -> Option<DirectCallResult> {
        None
    }
}

#[allow(dead_code)]
pub struct DirectCallArgs<'a> {
    pub name: &'a str,
    pub this: Option<&'a str>,
    pub args: &'a [&'a str],
}

#[allow(dead_code)]
pub enum DirectCallResult {
    Str(String),
    Int(i64),
    Bool(bool),
    Undefined,
}

/// An ordered set of abilities, built by chained `add` calls.
pub struct Registry {
    abilities: Vec<Ability>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            abilities: Vec::new(),
        }
    }

    /// Add a single ability.
    #[allow(dead_code)]
    pub fn add(mut self, ability: Ability) -> Self {
        self.abilities.push(ability);
        self
    }

    /// Add every ability from a static table, preserving order.
    pub fn add_all(mut self, abilities: &'static [Ability]) -> Self {
        self.abilities.extend_from_slice(abilities);
        self
    }

    /// Abilities of one phase, in eval order.
    pub fn phase(&self, phase: Phase) -> impl Iterator<Item = &Ability> {
        self.abilities.iter().filter(move |a| a.phase == phase)
    }

    /// Build the bootstrap source by concatenating each bootstrap
    /// ability's JS in eval order.
    pub fn bootstrap_source(&self) -> String {
        let mut source = String::new();
        for ability in self.phase(Phase::Bootstrap) {
            source.push_str(ability.js);
            source.push('\n');
        }
        source
    }

    /// Post-bootstrap abilities in eval order, as `(name, js)` pairs.
    pub fn post_bootstrap_fragments(&self) -> Vec<(&'static str, &'static str)> {
        self.phase(Phase::PostBootstrap)
            .map(|ability| (ability.name, ability.js))
            .collect()
    }
}

/// The full Node-compatible ability set, in eval order.
pub fn node_compat() -> Registry {
    Registry::new()
        .add_all(bootstrap::ABILITIES)
        .add_all(post_bootstrap::ABILITIES)
}

/// Declare an ability table: each `name => module` pair is written once
/// and expands to the module declaration, the ordered `ABILITIES` table,
/// and the name-based lookup. Data in, registry out.
macro_rules! abilities {
    ($phase:expr; $($name:literal => $module:ident),* $(,)?) => {
        $(pub mod $module;)*

        /// Abilities in eval order.
        pub const ABILITIES: &[crate::polyfills::Ability] = &[
            $(crate::polyfills::Ability { name: $name, phase: $phase, js: $module::JS }),*
        ];

        /// Look up an ability's JS bridge by name (with or without `.js`).
        #[allow(dead_code)]
        pub fn lookup(name: &str) -> Option<&'static str> {
            let bare = name.trim_end_matches(".js");
            ABILITIES
                .iter()
                .find(|ability| ability.name == bare)
                .map(|ability| ability.js)
        }
    };
}
pub mod bootstrap;
pub mod post_bootstrap;
