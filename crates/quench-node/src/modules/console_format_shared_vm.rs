//! `util.format` and `util.inspect` as the console uses them.
//!
//! `inspect` prefers Node's own (`node:util`) when it loads; otherwise default-option
//! inspection of plain data is the console inspector's exact rendering
//! (`console_shared_vm::inspect_flat`), and anything else is written structurally here:
//! primitives, functions, errors (their stack), symbols, custom inspectors, dates, regular
//! expressions, maps, sets, arrays and objects with their constructor's name, to a depth.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

/// `inspectDefaultOptions.depth`.
pub(crate) const DEFAULT_DEPTH: f64 = 2.0;
/// `inspectDefaultOptions.maxArrayLength`.
const DEFAULT_MAX_ARRAY_LENGTH: usize = 100;

/// The options of one inspection.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Options {
    /// Containers deeper than this are written as `[Type]`; `None` is unlimited.
    pub(crate) depth: Option<f64>,
    pub(crate) max_array_length: usize,
    pub(crate) layout: Layout,
    /// Calls `[util.inspect.custom]` methods.
    pub(crate) custom_inspect: bool,
    /// Writes hidden members: an array's `[length]`.
    pub(crate) show_hidden: bool,
    /// Writes a container's entries sorted.
    pub(crate) sorted: bool,
}

/// How containers break over lines.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layout {
    /// On one line when they fit `breakLength` (the default `compact: 3`).
    Fitted,
    /// Never broken (`breakLength: Infinity`).
    SingleLine,
    /// Every non-empty container one entry per line (`compact: false`).
    Expanded,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            depth: Some(DEFAULT_DEPTH),
            max_array_length: DEFAULT_MAX_ARRAY_LENGTH,
            layout: Layout::Fitted,
            custom_inspect: true,
            show_hidden: false,
            sorted: false,
        }
    }
}

impl Options {
    /// The JavaScript options object Node's `inspect` takes.
    fn object(&self, c: &mut Context<'_>) -> Result<RootId, RootedError> {
        let options = c.object_rooted()?;
        let depth = match self.depth {
            Some(depth) => c.number(depth),
            None => c.null(),
        };
        c.assign_named_property_rooted(options, "depth", depth)?;
        let maximum = c.number(self.max_array_length as f64);
        c.assign_named_property_rooted(options, "maxArrayLength", maximum)?;
        match self.layout {
            Layout::Fitted => {}
            Layout::SingleLine => {
                let infinity = c.number(f64::INFINITY);
                c.assign_named_property_rooted(options, "breakLength", infinity)?;
            }
            Layout::Expanded => {
                let no = c.boolean(false);
                c.assign_named_property_rooted(options, "compact", no)?;
            }
        }
        for (name, value) in [("customInspect", self.custom_inspect), ("showHidden", self.show_hidden), ("sorted", self.sorted)] {
            let value = c.boolean(value);
            c.assign_named_property_rooted(options, name, value)?;
        }
        Ok(options)
    }
}

/// `util.inspect(value, options)`, through the one inspector (`util_inspect_shared_vm`).
pub(crate) fn inspect(c: &mut Context<'_>, value: RootId, options: Options) -> Result<String, RootedError> {
    let object = options.object(c)?;
    let missing = c.undefined();
    let text = super::util_inspect_shared_vm::inspect(c, missing, &[value, object])?;
    c.to_string(text)
}















/// `util.format(...args)` with default inspection options, through the one formatter.
pub(crate) fn format(c: &mut Context<'_>, args: &[RootId]) -> Result<String, RootedError> {
    let units = super::util_inspect_shared_vm::format_with_options(c, None, args)?;
    Ok(String::from_utf16_lossy(&units))
}
