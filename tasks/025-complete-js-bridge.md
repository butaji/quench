# Task 025: Complete js_bridge.rs for All 89 Ink Features

**Priority:** P1-High  
**Phase:** 1 — rquickjs Dev Engine  
**Status:** ✅ COMPLETED
**ETA:** 6–8 hours  
**Depends on:** 024

## The Problem

`js_bridge.rs` currently supports only basic Box/Text props. Missing:
- `minWidth`, `minHeight`, `maxWidth`, `maxHeight`
- `zIndex`
- `overflowX`, `overflowY`
- `gap`, `columnGap`, `rowGap`
- `flexBasis`
- `position` + `top`/`right`/`bottom`/`left`
- `display="none"`
- Text: `wrap`, `truncate`
- All border side variants

## Steps

### Step 1: Audit all props used across 91 examples

```bash
grep -rhoE '\b[a-z][a-zA-Z]+=\{' examples/*/tui/app.tsx | sort | uniq -c | sort -rn
```

### Step 2: Implement missing prop setters

For each missing prop, add a setter in `js_bridge.rs`:

```rust
fn box_set_min_width(b: &mut InkBox, v: &Value) {
    if let Ok(n) = v.as_int() { b.min_width = Some(n as u16); }
}
fn box_set_max_width(b: &mut InkBox, v: &Value) { ... }
fn box_set_z_index(b: &mut InkBox, v: &Value) { ... }
// etc
```

### Step 3: Generate bridge registration dynamically

Instead of hand-writing every setter, generate from a table:

```rust
static BOX_PROP_TABLE: &[(&str, BoxSetter)] = &[
    ("width", box_set_width),
    ("height", box_set_height),
    ("minWidth", box_set_min_width),
    ("maxWidth", box_set_max_width),
    // ... 60 entries
];
```

Register in rquickjs:
```rust
for (name, setter) in BOX_PROP_TABLE {
    let name = *name;
    let func = Function::new(ctx.clone(), move |b: &mut InkBox, v: Value| {
        setter(b, &v);
    })?;
    props_obj.set(name, func)?;
}
```

### Step 4: Unit tests

Create `tests/js_bridge_props.rs`:
```rust
#[test]
fn test_box_min_width() {
    let bridge = InkBridge::new();
    let mut box = InkBox::new();
    bridge.apply_box_prop(&mut box, "minWidth", &Value::Number(10.0));
    assert_eq!(box.min_width, Some(10));
}
```

One test per prop.

## Acceptance Criteria

- [x] Every prop used in any of the 91 examples is supported.
- [x] `grep -r 'unsupported prop' tests/` returns nothing.
- [x] Unit test coverage for every prop setter.
