//! Host function families: one registered operation per family, dispatching on the member
//! index its function data leads with; the data's other entries are a closure's captures.

use crate::modules::stream_shared_vm::{get, Context, Outcome};
use quench_runtime::{RootId, RootedError};

/// A family of host functions sharing one operation, which dispatches on the member index
/// its function data leads with; the data's other entries are the function's captures.
pub(crate) trait Members: Copy + 'static {
    const OPERATION: &'static str;
    const ALL: &'static [Self];
    fn index(self) -> usize;
}

/// Declare a `Members` family: the enum, its operation name and its members in order.
macro_rules! members {
    ($name:ident, $operation:literal, [$($(#[$meta:meta])* $member:ident),* $(,)?]) => {
        #[derive(Clone, Copy, PartialEq, Eq)]
        pub(crate) enum $name { $($(#[$meta])* $member),* }
        impl $crate::modules::host_members::Members for $name {
            const OPERATION: &'static str = $operation;
            const ALL: &'static [Self] = &[$(Self::$member),*];
            fn index(self) -> usize { self as usize }
        }
    };
}
pub(crate) use members;

/// Member `member`'s function named `name`, closing over `captures`.
pub(crate) fn function<M: Members>(c: &mut Context<'_>, member: M, name: &str, captures: &[RootId]) -> Outcome {
    let data = member_data(c, member, captures)?;
    c.host_function_named_with_data(crate::host::shared_vm::operation(M::OPERATION), name, data)
}

/// Member `member` as a constructor named `name`, with a fresh `prototype`.
pub(crate) fn constructor<M: Members>(c: &mut Context<'_>, member: M, name: &str) -> Outcome {
    let data = member_data(c, member, &[])?;
    c.host_constructor_named_with_data(crate::host::shared_vm::operation(M::OPERATION), name, data)
}

fn member_data<M: Members>(c: &mut Context<'_>, member: M, captures: &[RootId]) -> Outcome {
    let mut values = Vec::with_capacity(captures.len() + 1);
    values.push(c.number(member.index() as f64));
    values.extend_from_slice(captures);
    c.array_rooted(&values)
}

/// The member the running function is.
pub(crate) fn current<M: Members>(c: &mut Context<'_>) -> Result<M, RootedError> {
    let data = c.host_function_data()?;
    let index = get(c, data, "0")?;
    let index = c.rooted_value(index).and_then(|value| value.as_number()).unwrap_or(-1.0);
    M::ALL
        .get(index as usize)
        .copied()
        .ok_or_else(|| RootedError::host("http member index out of range"))
}

/// The running function's captures.
pub(crate) fn captured<const N: usize>(c: &mut Context<'_>) -> Result<[RootId; N], RootedError> {
    let data = c.host_function_data()?;
    let values = c.array_like_values_rooted(data)?;
    let missing = c.undefined();
    Ok(std::array::from_fn(|index| values.get(index + 1).copied().unwrap_or(missing)))
}

/// Methods as Node assigns them to a prototype: enumerable, writable and configurable.
pub(crate) fn methods<M: Members>(c: &mut Context<'_>, target: RootId, members: &[(&str, M)]) -> Result<(), RootedError> {
    for &(name, member) in members {
        let value = function(c, member, name, &[])?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(target, key, value, true, true, true)?;
    }
    Ok(())
}

/// Accessors as `ObjectDefineProperty(prototype, name, { get, set })` installs them.
pub(crate) fn accessors<M: Members>(c: &mut Context<'_>, target: RootId, members: &[(&str, M, Option<M>)]) -> Result<(), RootedError> {
    for &(name, getter, setter) in members {
        let get = function(c, getter, &format!("get {name}"), &[])?;
        let set = match setter {
            Some(setter) => function(c, setter, &format!("set {name}"), &[])?,
            None => c.undefined(),
        };
        let key = c.string_rooted(name);
        let fields = quench_runtime::DescriptorFields {
            get: Some(get),
            set: Some(set),
            enumerable: Some(false),
            configurable: Some(true),
            ..Default::default()
        };
        c.define_property_fields_rooted(target, key, fields)?;
    }
    Ok(())
}
