# Task 305: Implement Rest Parameter Binding

## Status: COMPLETED

## Summary

Rest parameters (`...args`) now work correctly in both regular and arrow functions.

## Changes Made

### AST Changes (`crates/quench-runtime/src/ast.rs`)

Added `Param` enum to distinguish regular vs rest parameters:

```rust
/// Function parameter - either regular or rest
#[derive(Debug, Clone, PartialEq)]
pub enum Param {
    /// Regular parameter: `x`
    Regular(String),
    /// Rest parameter: `...args`
    Rest(String),
}

impl Param {
    /// Get the name of this parameter
    pub fn name(&self) -> &str { ... }
    
    /// Returns true if this is a rest parameter
    pub fn is_rest(&self) -> bool { ... }
}
```

Updated function types to use `Vec<Param>` instead of `Vec<String>`:
- `Statement::FunctionDeclaration`
- `Expression::FunctionExpression`
- `Expression::ArrowFunction`

### Lowering Changes (`crates/quench-runtime/src/lower.rs`)

Added `lower_param()` helper function:

```rust
fn lower_param(pat: &swc::Pat) -> Param {
    match pat {
        swc::Pat::Ident(ident) => Param::Regular(atom_to_string(&ident.id.sym)),
        swc::Pat::Rest(rest) => {
            if let swc::Pat::Ident(ident) = rest.arg.as_ref() {
                Param::Rest(atom_to_string(&ident.id.sym))
            } else {
                Param::Regular("__rest".to_string())
            }
        }
        // ...
    }
}
```

Updated all function parameter lowering sites to use `lower_param()`.

### Interpreter Changes (`crates/quench-runtime/src/interpreter.rs`)

Added `bind_function_params()` helper:

```rust
pub fn bind_function_params(
    env: &mut Environment,
    params: &[Param],
    args: &[Value],
) {
    let rest_param_idx = params.iter().position(|p| p.is_rest());
    
    match rest_param_idx {
        Some(rest_idx) => {
            // Bind regular parameters, collect rest into array
            let rest_args: Vec<Value> = args.iter()
                .skip(rest_idx)
                .cloned()
                .collect();
            env.define(rest_param_name, create_array_from_values(&rest_args));
        }
        None => {
            // Bind all parameters by position
        }
    }
}
```

Also fixed computed property access in `lower_member_prop()`:

```rust
swc::MemberProp::Computed(computed) => {
    let expr = lower_expr(&computed.expr)?;
    Ok((PropertyKey::Computed(Box::new(expr)), true))
}
```

## Tests Added

All 8 rest parameter tests now pass:

1. `test_rest_parameter_basic` - Basic rest parameter collection
2. `test_rest_parameter_empty` - Rest with no arguments
3. `test_rest_parameter_single` - Rest with single argument
4. `test_rest_parameter_with_prefix_params` - Rest after regular params
5. `test_rest_parameter_access_elements` - Array element access
6. `test_arrow_function_rest_parameter` - Arrow function rest params
7. `test_arrow_function_rest_parameter_with_prefix` - Arrow with prefix
8. `test_rest_parameter_in_nested_function` - Nested function rest params

## Acceptance Criteria

- ✅ All 8 rest parameter tests pass
- ✅ Basic rest: `function test(...args) { return args; }`
- ✅ Rest with prefix: `function test(a, b, ...rest) { return rest; }`
- ✅ Arrow rest: `const test = (...args) => args`
- ✅ Element access: `args[0]`, `args[1]`
