//! AbortSignal.any dependency edges. Both endpoints are genuinely weak; the
//! composite's own state stores the abort snapshot before any dispatch or GC.
use super::event_constructor_shared_vm::get;
use super::events_shared_vm::listener_records;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type C<'a> = NativeContext<'a, NodeHost>;
fn put(c: &mut C<'_>, o: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    if c.define_data_property_rooted(o, key, value, true, true, true)? {
        Ok(())
    } else {
        Err(RootedError::host("cannot update private abort dependency"))
    }
}
fn call(
    c: &mut C<'_>,
    d: RootId,
    name: &str,
    receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let f = get(c, d, name)?;
    c.call_rooted(f, receiver, args)
}
fn weak(c: &mut C<'_>, d: RootId, target: RootId) -> Result<RootId, RootedError> {
    let ctor = get(c, d, "weakRefCtor")?;
    c.construct_rooted(ctor, ctor, &[target])
}
fn deref(c: &mut C<'_>, d: RootId, reference: RootId) -> Result<RootId, RootedError> {
    call(c, d, "weakDeref", reference, &[])
}
fn absent(c: &C<'_>, v: RootId) -> bool {
    c.rooted_value(v).is_some_and(|v| v.is_undefined())
}
fn state(c: &mut C<'_>, d: RootId, signal: RootId) -> Result<RootId, RootedError> {
    let map = get(c, d, "abortStateMap")?;
    c.weak_map_get_rooted(map, signal)
}
fn edges(c: &mut C<'_>, state: RootId) -> Result<Vec<RootId>, RootedError> {
    let a = get(c, state, "parents")?;
    listener_records(c, a)
}
pub(crate) fn initialize(c: &mut C<'_>, d: RootId) -> Result<(), RootedError> {
    for (field, operation) in [
        ("abortSourceRegistry", "abortSourceCollected"),
        ("abortDependentRegistry", "abortDependentCollected"),
    ] {
        let f = c.host_function_with_data(crate::host::shared_vm::operation(operation), d)?;
        let ctor = get(c, d, "weakRegistryCtor")?;
        let registry = c.construct_rooted(ctor, ctor, &[f])?;
        put(c, d, field, registry)?;
    }
    Ok(())
}
/// Original SDK sequence conversion and native map brand own all input policy.
pub(crate) fn convert(c: &mut C<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let d = c.host_function_data()?;
    let input = args.first().copied().unwrap_or_else(|| c.undefined());
    let mut converter = get(c, d, "abortSequenceConverter")?;
    if absent(c, converter) {
        let module = crate::host::shared_vm::commonjs::node_library_module(
            c,
            "internal/abort_controller.js",
            "internal/webidl.js",
        )?;
        let factory = get(c, module, "createSequenceConverter")?;
        let converters = get(c, module, "converters")?;
        let item = get(c, converters, "AbortSignal")?;
        let u = c.undefined();
        converter = c.call_rooted(factory, u, &[item])?;
        put(c, d, "abortSequenceConverter", converter)?;
    }
    let options = c.null_object_rooted()?;
    let name = c.string_rooted("signals");
    put(c, options, "context", name)?;
    let u = c.undefined();
    c.call_rooted(converter, u, &[input, options])
}
/// Flatten to original sources, retaining only their WeakRefs after this call.
pub(crate) fn link(c: &mut C<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let d = c.host_function_data()?;
    let result = args[0];
    let inputs = args[1];
    let rs = state(c, d, result)?;
    let yes = c.boolean(true);
    put(c, rs, "composite", yes)?;
    let mut sources = Vec::new();
    for signal in listener_records(c, inputs)? {
        super::abort_timeout_shared_vm::compose_source(c, d, signal, rs)?;
        let ss = state(c, d, signal)?;
        let aborted = get(c, ss, "aborted")?;
        if c.truthy_rooted(aborted)? {
            let reason = get(c, ss, "reason")?;
            let abort = get(c, d, "abortSignal")?;
            let undefined = c.undefined();
            c.call_rooted(abort, undefined, &[result, reason])?;
            return Ok(result);
        }
        let composite = get(c, ss, "composite")?;
        let candidates = if c.truthy_rooted(composite)? {
            let mut values = Vec::new();
            for edge in edges(c, ss)? {
                let r = get(c, edge, "source")?;
                let source = deref(c, d, r)?;
                if !absent(c, source) {
                    values.push(source);
                }
            }
            values
        } else {
            vec![signal]
        };
        for source in candidates {
            let mut duplicate = false;
            for old in &sources {
                if c.strict_equal_rooted(*old, source)? {
                    duplicate = true;
                    break;
                }
            }
            if !duplicate {
                sources.push(source);
            }
        }
    }
    let result_ref = weak(c, d, result)?;
    let mut links = Vec::new();
    for source in sources {
        let source_ref = weak(c, d, source)?;
        let edge = c.null_object_rooted()?;
        put(c, edge, "source", source_ref)?;
        put(c, edge, "dependent", result_ref)?;
        let ss = state(c, d, source)?;
        let children = get(c, ss, "children")?;
        call(c, d, "weakSetAdd", children, &[result_ref])?;
        let registry = get(c, d, "abortSourceRegistry")?;
        // Give each source edge its own unregister token. A dependent may
        // flatten to several sources, and unregistering by the shared result
        // WeakRef from one edge would silently cancel every other source
        // cleanup registration for that dependent.
        call(c, d, "weakRegister", registry, &[source, edge, edge])?;
        let registry = get(c, d, "abortDependentRegistry")?;
        call(c, d, "weakRegister", registry, &[result, edge, edge])?;
        links.push(edge);
    }
    let links = c.array_rooted(&links)?;
    put(c, rs, "parents", links)?;
    Ok(result)
}
/// Sources retain observed dependents, but no dependent retains its sources.
pub(crate) fn refresh(c: &mut C<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let d = c.host_function_data()?;
    let signal = args[0];
    let s = args[1];
    let observed = super::abort_timeout_shared_vm::refresh(c, d, signal, s)?;
    for edge in edges(c, s)? {
        let r = get(c, edge, "source")?;
        let source = deref(c, d, r)?;
        if absent(c, source) {
            continue;
        }
        let ss = state(c, d, source)?;
        let dependents = get(c, ss, "dependents")?;
        call(
            c,
            d,
            if observed {
                "weakSetAdd"
            } else {
                "weakSetDelete"
            },
            dependents,
            &[signal],
        )?;
    }
    Ok(c.undefined())
}
fn detach_source(c: &mut C<'_>, d: RootId, edge: RootId) -> Result<(), RootedError> {
    let source_ref = get(c, edge, "source")?;
    let source = deref(c, d, source_ref)?;
    if absent(c, source) {
        return Ok(());
    }
    let ss = state(c, d, source)?;
    let children = get(c, ss, "children")?;
    let dependent_ref = get(c, edge, "dependent")?;
    call(c, d, "weakSetDelete", children, &[dependent_ref])?;
    let dependent = deref(c, d, dependent_ref)?;
    if !absent(c, dependent) {
        let observed = get(c, ss, "dependents")?;
        call(c, d, "weakSetDelete", observed, &[dependent])?;
    }
    Ok(())
}
/// Called only after abort state/reason have been copied to every descendant.
pub(crate) fn settle(c: &mut C<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let d = c.host_function_data()?;
    let signal = args[0];
    let s = args[1];
    let links = edges(c, s)?;
    super::abort_timeout_shared_vm::release(c, d, signal)?;
    for edge in links {
        detach_source(c, d, edge)?;
        let registry = get(c, d, "abortDependentRegistry")?;
        call(c, d, "weakUnregister", registry, &[edge])?;
        let registry = get(c, d, "abortSourceRegistry")?;
        call(c, d, "weakUnregister", registry, &[edge])?;
    }
    let empty = c.array_rooted(&[])?;
    put(c, s, "parents", empty)?;
    Ok(c.undefined())
}
pub(crate) fn dependent_collected(
    c: &mut C<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let d = c.host_function_data()?;
    let edge = args[0];
    detach_source(c, d, edge)?;
    let registry = get(c, d, "abortSourceRegistry")?;
    call(c, d, "weakUnregister", registry, &[edge])?;
    Ok(c.undefined())
}
pub(crate) fn source_collected(
    c: &mut C<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let d = c.host_function_data()?;
    let edge = args[0];
    let r = get(c, edge, "dependent")?;
    let dependent = deref(c, d, r)?;
    if !absent(c, dependent) {
        let s = state(c, d, dependent)?;
        let mut retained = Vec::new();
        for candidate in edges(c, s)? {
            if !c.strict_equal_rooted(candidate, edge)? {
                retained.push(candidate);
            }
        }
        if retained.is_empty() {
            super::abort_timeout_shared_vm::release(c, d, dependent)?;
        }
        let a = c.array_rooted(&retained)?;
        put(c, s, "parents", a)?;
    }
    let registry = get(c, d, "abortDependentRegistry")?;
    call(c, d, "weakUnregister", registry, &[edge])?;
    Ok(c.undefined())
}
