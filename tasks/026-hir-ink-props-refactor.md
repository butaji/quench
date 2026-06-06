# Task 026: Refactor apply_box_prop into a Dispatch Table or Macro

**Priority:** P1-High  
**Phase:** 2 — HIR Runtime Core Engine  
**ETA:** 2 hours  
**Depends on:** 022

## The Problem

`apply_box_prop` in `src/hir_runtime.rs` is **309 lines** with **complexity 147**.

It handles every Ink CSS property with copy-pasted boilerplate:

```rust
"paddingTop" => {
    if let Value::Number(n) = val {
        b.padding_top = Some(*n as u16);
    }
}
"paddingBottom" => {
    if let Value::Number(n) = val {
        b.padding_bottom = Some(*n as u16);
    }
}
// ... repeated 40+ times
```

Adding a new property (e.g. `minWidth`, `maxWidth`, `zIndex`) requires finding the right spot and pasting another 4-line block. This is error-prone and scales linearly with the number of properties.

## Why This Matters

- Ink has ~60 CSS properties. The current approach is unmaintainable.
- The linter limit is 40 lines / 10 complexity. This function is 7× over.
- EXECUTE.md requires covering "all of Ink features." We need to add more properties, not fewer.

## Steps

### Step 1: Define property metadata

Create a static table mapping property names to setters:

```rust
type BoxSetter = fn(&mut InkBox, &Value);

static BOX_PROPS: phf::Map<&'static str, BoxSetter> = phf::phf_map! {
    "paddingTop" => |b, v| set_u16!(b.padding_top, v),
    "paddingBottom" => |b, v| set_u16!(b.padding_bottom, v),
    "paddingLeft" => |b, v| set_u16!(b.padding_left, v),
    "paddingRight" => |b, v| set_u16!(b.padding_right, v),
    "padding" => |b, v| set_uniform_u16!(b, padding_top, padding_right, padding_bottom, padding_left, v),
    "paddingX" => |b, v| set_uniform_u16!(b, padding_left, padding_right, v),
    "paddingY" => |b, v| set_uniform_u16!(b, padding_top, padding_bottom, v),
    // ... etc
};
```

If `phf` is too heavy, use a `match` but generate it with a macro:

```rust
macro_rules! box_prop_match {
    ($b:expr, $key:expr, $val:expr) => {{
        match $key {
            "paddingTop" => set_u16!($b.padding_top, $val),
            "paddingBottom" => set_u16!($b.padding_bottom, $val),
            // generated
            _ => {}
        }
    }};
}
```

### Step 2: Write helper macros

```rust
macro_rules! set_u16 {
    ($field:expr, $val:expr) => {
        if let Value::Number(n) = $val {
            $field = Some(*n as u16);
        }
    };
}

macro_rules! set_uniform_u16 {
    ($b:expr, $($field:ident),+, $val:expr) => {
        if let Value::Number(n) = $val {
            let v = Some(*n as u16);
            $($b.$field = v;)*
        }
    };
}

macro_rules! set_enum_prop {
    ($b:expr, $field:ident, $val:expr, $enum:ident, { $($pat:expr => $variant:expr),* }) => {
        if let Value::String(s) = $val {
            $b.$field = match s.as_str() {
                $($pat => $variant,)*
                _ => return,
            };
        }
    };
}
```

### Step 3: Rewrite `apply_box_prop`

```rust
fn apply_box_prop(b: &mut InkBox, key: &str, val: &Value) {
    box_prop_match!(b, key, val);
}
```

This function is now **≤ 5 lines**.

### Step 4: Do the same for `apply_text_prop`

```rust
fn apply_text_prop(t: &mut InkText, key: &str, val: &Value) {
    text_prop_match!(t, key, val);
}
```

### Step 5: Add missing properties

After the refactor, adding `minWidth` is one line:

```rust
"minWidth" => |b, v| set_u16!(b.min_width, v),
```

Add these missing Ink properties:
- `minWidth`, `minHeight`
- `maxWidth`, `maxHeight`
- `zIndex`

## Acceptance Criteria

- [ ] `apply_box_prop` ≤ 40 lines, complexity ≤ 10.
- [ ] `apply_text_prop` ≤ 40 lines, complexity ≤ 10.
- [ ] All existing examples render identically (no regression).
- [ ] Adding a new property requires editing exactly one line in a table.
- [ ] Unit test: every property in the table is exercised at least once.

## Notes

- If macros feel too magical, use a `HashMap<&str, BoxSetter>` built at module init. Performance is irrelevant here — it's called once per JSX element.
- The macro approach is zero-cost and compile-time validated.
