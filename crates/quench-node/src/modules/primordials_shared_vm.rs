//! The intrinsic property copies of Node's `primordials.js`, run natively.
//!
//! `primordials.js` copies every own property of about fifty intrinsics onto `primordials`
//! through three functions (`copyPropsRenamed`, `copyPropsRenamedBound`, `copyPrototype`).
//! Interpreted, each of its ~1,700 properties builds a template key, a descriptor object and
//! its spread, and searches `varargsMethods`: about 40% of a `require('util')` run. The
//! projected module keeps every other statement and replaces only those three declarations
//! with calls into `copy`, which makes the same definitions in the same order: the same
//! renamed keys, descriptor fields, `uncurryThis`, `applyBind` and `bind` wrappers, and the
//! same `Reflect.defineProperty` results.

use crate::host::NodeHost;
use quench_runtime::{DescriptorFields, NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;

/// `require(COPY_REQUEST)` from the projected `primordials.js`: the native copy.
pub(crate) const COPY_REQUEST: &str = "internal/quench/primordials_copy";

/// The binding the projection gives the native copy.
const COPY_BINDING: &str = "copyIntrinsicProperties";

/// How each of `primordials.js`'s copy functions wraps a copied data property's function value.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CopyKind {
    /// `copyPropsRenamed`: the value as is; its `Apply` variant binds `src` as `this`.
    Renamed,
    /// `copyPropsRenamedBound`: `value.bind(src)`; its `Apply` variant binds `src` as `this`.
    RenamedBound,
    /// `copyPrototype`: `uncurryThis(value)`; its `Apply` variant binds no `this`.
    Prototype,
}

/// The replaced declarations, by name: the projection and the native dispatch both read it.
const COPIES: [(&str, CopyKind); 3] = [
    ("copyPropsRenamed", CopyKind::Renamed),
    ("copyPropsRenamedBound", CopyKind::RenamedBound),
    ("copyPrototype", CopyKind::Prototype),
];

/// The description prefix of a well-known symbol (`Symbol.iterator`), which `getNewKey`
/// drops: it upper-cases the code unit after it.
const WELL_KNOWN_SYMBOL_PREFIX: usize = "Symbol.".len();

/// The projected `primordials.js`: each copy declaration calls the native copy with the
/// module's own `varargsMethods`, `uncurryThis` and `applyBind`. Every other statement keeps
/// its bytes, and each replacement keeps its declaration's line count, so later positions
/// (stack traces) are unchanged.
pub(crate) fn project_primordials(source: &str) -> Result<String, RootedError> {
    let allocator = oxc_allocator::Allocator::default();
    let parsed =
        oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::cjs()).parse();
    if !parsed.diagnostics.is_empty() {
        return Err(RootedError::host("primordials projection: the library does not parse"));
    }
    let mut spans = Vec::new();
    for statement in &parsed.program.body {
        let oxc_ast::ast::Statement::FunctionDeclaration(function) = statement else {
            continue;
        };
        if let Some(id) = function.id.as_ref().filter(|id| COPIES.iter().any(|(name, _)| id.name == *name)) {
            spans.push((function.span.start as usize, function.span.end as usize, id.name.as_str()));
        }
    }
    let mut names = spans.iter().map(|(_, _, name)| *name).collect::<Vec<_>>();
    names.sort_unstable();
    let mut expected = COPIES.iter().map(|(name, _)| *name).collect::<Vec<_>>();
    expected.sort_unstable();
    if names != expected {
        return Err(RootedError::host(
            "primordials projection expects each copy declaration once",
        ));
    }
    let mut projected = String::with_capacity(source.len());
    let mut copied = 0;
    for (index, (start, end, name)) in spans.into_iter().enumerate() {
        projected.push_str(&source[copied..start]);
        // The first declaration also binds the native copy; all calls follow it.
        if index == 0 {
            projected.push_str(&format!("const {COPY_BINDING} = require('{COPY_REQUEST}'); "));
        }
        let replacement = format!(
            "function {name}(src, dest, prefix) {{ {COPY_BINDING}('{name}', src, dest, prefix, \
             varargsMethods, uncurryThis, applyBind); }}"
        );
        projected.push_str(&replacement);
        projected.push_str(&"\n".repeat(source[start..end].matches('\n').count()));
        copied = end;
    }
    projected.push_str(&source[copied..]);
    Ok(projected)
}

/// `require(COPY_REQUEST)`: the native copy function.
pub(crate) fn copy_request(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    c.host_function_named(crate::host::shared_vm::operation("primordialsCopy"), COPY_BINDING)
}

/// `copyIntrinsicProperties(kind, src, dest, prefix, varargsMethods, uncurryThis, applyBind)`:
/// the body of the copy declaration named `kind`.
pub(crate) fn copy(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let [kind, src, dest, prefix, varargs, uncurry_this, apply_bind] = args else {
        return Err(RootedError::host("primordials copy takes seven arguments"));
    };
    let kind = c.string_text(*kind)?;
    let Some(&(_, kind)) = COPIES.iter().find(|(name, _)| Some(*name) == kind.as_deref()) else {
        return Err(RootedError::host("primordials copy names no copy declaration"));
    };
    let copier = Copier {
        kind,
        src: *src,
        dest: *dest,
        prefix: c.to_string_code_units(*prefix)?,
        varargs: string_elements(c, *varargs)?,
        uncurry_this: *uncurry_this,
        apply_bind: *apply_bind,
    };
    for key in c.own_keys_rooted(copier.src)? {
        let mark = c.root_scope_mark();
        copier.copy_property(c, key)?;
        c.release_roots_since(mark, &[]);
    }
    Ok(c.undefined())
}

struct Copier {
    kind: CopyKind,
    src: RootId,
    dest: RootId,
    prefix: Vec<u16>,
    /// `varargsMethods`, whose names also get an `Apply` variant.
    varargs: Vec<Vec<u16>>,
    uncurry_this: RootId,
    apply_bind: RootId,
}

impl Copier {
    /// One iteration of the copy loop: `getNewKey`, the descriptor read, then
    /// `copyAccessor` or the data property and its `Apply` variant.
    fn copy_property(&self, c: &mut Context<'_>, key: RootId) -> Result<(), RootedError> {
        let new_key = new_key(c, key)?;
        let Some(descriptor) = c.own_property_fields_rooted(self.src, key)? else {
            let error = c.type_error_rooted("Cannot use 'in' operator to search for 'get' in undefined")?;
            return Err(c.throw(error));
        };
        // `'get' in desc`: a descriptor object has `get` exactly when it describes an accessor.
        if let Some(get) = descriptor.get {
            return self.copy_accessor(c, &new_key, get, descriptor.set, descriptor.enumerable);
        }
        let Some(value) = descriptor.value else {
            return Err(RootedError::host("a data descriptor has a value"));
        };
        let wrapped = if c.is_callable_rooted(value)? {
            match self.kind {
                CopyKind::Renamed => value,
                CopyKind::RenamedBound => {
                    let bind = c.get_named_property_rooted(value, "bind")?;
                    c.call_rooted(bind, value, &[self.src])?
                }
                CopyKind::Prototype => {
                    let undefined = c.undefined();
                    c.call_rooted(self.uncurry_this, undefined, &[value])?
                }
            }
        } else {
            value
        };
        let name = [self.prefix.as_slice(), &new_key].concat();
        self.define(c, &name, DescriptorFields { value: Some(wrapped), ..descriptor })?;
        if self.varargs.contains(&name) {
            let undefined = c.undefined();
            let applied = match self.kind {
                CopyKind::Renamed | CopyKind::RenamedBound => {
                    c.call_rooted(self.apply_bind, undefined, &[value, self.src])?
                }
                CopyKind::Prototype => c.call_rooted(self.apply_bind, undefined, &[value])?,
            };
            let name = [name.as_slice(), &units("Apply")].concat();
            self.define(c, &name, DescriptorFields { value: Some(applied), ..DescriptorFields::default() })?;
        }
        Ok(())
    }

    /// `copyAccessor(dest, prefix, key, { enumerable, get, set })`.
    fn copy_accessor(
        &self,
        c: &mut Context<'_>,
        key: &[u16],
        get: RootId,
        set: Option<RootId>,
        enumerable: Option<bool>,
    ) -> Result<(), RootedError> {
        let undefined = c.undefined();
        let set = set.filter(|set| c.rooted_value(*set).is_some_and(|set| !set.is_undefined()));
        for (accessor, verb) in [(Some(get), "Get"), (set, "Set")] {
            let Some(accessor) = accessor else { continue };
            let value = c.call_rooted(self.uncurry_this, undefined, &[accessor])?;
            let name = [self.prefix.as_slice(), &units(verb), key].concat();
            self.define(c, &name, DescriptorFields { value: Some(value), enumerable, ..DescriptorFields::default() })?;
        }
        Ok(())
    }

    /// `ReflectDefineProperty(dest, name, descriptor)`, whose result the copies ignore.
    fn define(&self, c: &mut Context<'_>, name: &[u16], fields: DescriptorFields) -> Result<(), RootedError> {
        let name = c.string_units_rooted(name);
        c.define_property_fields_rooted(self.dest, name, fields)?;
        Ok(())
    }
}

/// `getNewKey(key)`: a well-known symbol's name after `Symbol.`, or a string key, with its
/// first code unit upper-cased; a symbol's gains the `Symbol` prefix.
fn new_key(c: &mut Context<'_>, key: RootId) -> Result<Vec<u16>, RootedError> {
    let (prefix, name, first) = if c.is_symbol_rooted(key)? {
        let description = c.get_named_property_rooted(key, "description")?;
        (units("Symbol"), c.string_code_units(description)?, WELL_KNOWN_SYMBOL_PREFIX)
    } else {
        (Vec::new(), c.string_code_units(key)?, 0)
    };
    let Some((&unit, rest)) = name.as_ref().and_then(|name| name.get(first..)?.split_first()) else {
        let error = c.type_error_rooted("Cannot read properties of undefined (reading 'toUpperCase')")?;
        return Err(c.throw(error));
    };
    Ok([prefix.as_slice(), &upper_case_code_unit(unit), rest].concat())
}

/// `String.prototype.toUpperCase` of a one-code-unit string: a lone surrogate is unchanged.
fn upper_case_code_unit(unit: u16) -> Vec<u16> {
    match char::from_u32(u32::from(unit)) {
        Some(character) => character.to_uppercase().collect::<String>().encode_utf16().collect(),
        None => vec![unit],
    }
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// The string elements of an array of strings (`varargsMethods`).
fn string_elements(c: &mut Context<'_>, array: RootId) -> Result<Vec<Vec<u16>>, RootedError> {
    let mut elements = Vec::new();
    let iterator = c.iterator_rooted(array)?;
    while let Some(element) = c.iterator_step_rooted(&iterator)? {
        if let Some(units) = c.string_code_units(element)? {
            elements.push(units);
        }
    }
    Ok(elements)
}
