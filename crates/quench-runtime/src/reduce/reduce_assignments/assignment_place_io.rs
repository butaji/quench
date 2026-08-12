use super::{take_register, Place, PlaceKey};
use crate::ops::Op;

pub(crate) fn get(place: &Place, ops: &mut Vec<Op>, next: &mut u16) -> Option<u16> {
    let dst = take_register(next);
    match place {
        Place::Local { slot } => ops.push(Op::LoadLocal { dst, slot: *slot }),
        Place::DynamicLocal {
            name, slot, target, ..
        } => emit_dynamic_local_get(ops, dst, name, *slot, *target),
        Place::Name { name, .. } => ops.push(Op::ResolveName {
            dst,
            key: name.clone(),
        }),
        Place::Property { object, key, .. } => emit_property_get(ops, dst, *object, key),
        Place::Private { object, name } => ops.push(Op::GetPrivate {
            dst,
            object: *object,
            name: *name,
        }),
        Place::Super {
            key: PlaceKey::Static(key),
        } => ops.push(Op::GetSuperProperty {
            dst,
            key: key.clone(),
        }),
        Place::Super {
            key: PlaceKey::Dynamic(key),
        } => ops.push(Op::GetSuperPropertyDynamic { dst, key: *key }),
    }
    Some(dst)
}

fn emit_dynamic_local_get(ops: &mut Vec<Op>, dst: u16, name: &str, slot: u16, target: Option<u16>) {
    match target {
        Some(target) => ops.push(Op::LoadResolvedLocalBinding {
            dst,
            target,
            slot,
            name: name.to_string(),
        }),
        None => ops.push(Op::LoadBinding {
            dst,
            slot,
            name: name.to_string(),
        }),
    }
}

fn emit_property_get(ops: &mut Vec<Op>, dst: u16, object: u16, key: &PlaceKey) {
    match key {
        PlaceKey::Static(key) => ops.push(Op::GetProperty {
            dst,
            object,
            key: key.clone(),
        }),
        PlaceKey::Dynamic(key) => ops.push(Op::GetPropertyDynamic {
            dst,
            object,
            key: *key,
        }),
    }
}

pub(crate) fn put(place: Place, value: u16, ops: &mut Vec<Op>) -> Option<()> {
    match place {
        Place::Local { slot } => put_local(ops, slot, value),
        Place::DynamicLocal {
            name,
            slot,
            strict,
            target: Some(target),
        } => ops.push(Op::SetResolvedLocalBinding {
            target,
            name,
            slot,
            strict,
            src: value,
        }),
        Place::DynamicLocal { name, strict, .. } => ops.push(Op::SetName {
            key: name,
            src: value,
            strict,
        }),
        Place::Name {
            name,
            strict,
            target,
        } => put_name(ops, name, strict, target, value),
        Place::Property {
            object,
            key,
            strict,
        } => emit_property_put(ops, object, key, value, strict),
        Place::Private { object, name } => ops.push(Op::SetPrivate {
            object,
            name,
            src: value,
        }),
        Place::Super { key } => emit_super_put(ops, key, value),
    }
    Some(())
}

fn put_local(ops: &mut Vec<Op>, slot: u16, value: u16) {
    ops.push(Op::CheckInitialized {
        slot,
        name: format!("local_{slot}"),
    });
    ops.push(Op::StoreLocal { slot, src: value });
}

fn put_name(ops: &mut Vec<Op>, name: String, strict: bool, target: Option<u16>, src: u16) {
    match target {
        Some(target) => ops.push(Op::SetResolvedBinding {
            target,
            name,
            src,
            strict,
        }),
        None => ops.push(Op::SetName {
            key: name,
            src,
            strict,
        }),
    }
}

fn emit_super_put(ops: &mut Vec<Op>, key: PlaceKey, value: u16) {
    match key {
        PlaceKey::Static(key) => ops.push(Op::SetSuperProperty { key, src: value }),
        PlaceKey::Dynamic(key) => ops.push(Op::SetSuperPropertyDynamic { key, src: value }),
    }
}

fn emit_property_put(ops: &mut Vec<Op>, object: u16, key: PlaceKey, value: u16, strict: bool) {
    match key {
        PlaceKey::Static(key) => ops.push(Op::SetProperty {
            object,
            key,
            src: value,
            strict,
        }),
        PlaceKey::Dynamic(key) => ops.push(Op::SetPropertyDynamic {
            object,
            key,
            src: value,
            strict,
        }),
    }
}
