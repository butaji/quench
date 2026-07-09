# Stack Machine Split Plan

## Current State
- `crates/quench-runtime/src/stack_machine.rs` - 1679 lines
- Exceeds 500 line limit by ~1180 lines

## Split Modules

### 1. `stack_machine/mod.rs` (~200 lines)
- Module documentation
- `eval_program()` function
- `Machine` struct definition
- `new()`, `run_statements()`, `run()`, `current_frame()`, `push_stmt_list()`, `pop_value()` methods
- Imports from submodules

### 2. `stack_machine/work.rs` (~200 lines)
- `Work` enum definition (all variants)
- `ForPhase` enum
- `ObjectPropertyKind` enum
- `AssignmentTarget` enum
- `CatchFrame` struct (or keep in frames.rs)

### 3. `stack_machine/frames.rs` (~100 lines)
- `Frame` struct definition
- `CatchFrame` struct

### 4. `stack_machine/step.rs` (~300 lines)
- `step()` method implementation
- Matches on Work variants and delegates

### 5. `stack_machine/eval_expr.rs` (~400 lines)
- `eval_expr()` method
- `eval_identifier()` method
- `eval_object()` method
- `eval_array()` method

### 6. `stack_machine/eval_stmt.rs` (~300 lines)
- `eval_stmt()` method
- `eval_stmts()` method

### 7. `stack_machine/apply.rs` (~500 lines)
- `apply_*` methods for all operations
- `apply_binary()`, `apply_unary()`, `apply_assign()`
- `apply_member()`, `apply_call()`, `apply_conditional()`
- `apply_update()`, `apply_new()`, `apply_constructor_result()`
- `apply_sequence()`, `apply_block_expr()`
- `apply_if()`, `apply_while()`, `apply_while_body()`
- `apply_for()`, `apply_for_body()`, `apply_block()`
- `apply_try_catch()`, `apply_return()`
- `apply_for_of()`, `apply_for_in()`
- `apply_object_property()`
- `eval_assignment()`, `apply_member_assign()`
- `apply_compound_assign()`, `eval_callee()`
- `create_arguments_object()`, `call_setter()`
- `var_decl()`, `for_init_var()`
- `begin_for_of()`, `begin_for_in()`

### 8. `stack_machine/helpers.rs` (~150 lines)
- `property_key_static()` function
- Helper traits/types

## Implementation Order
1. Create `stack_machine/` directory
2. Create `work.rs` with Work enum
3. Create `frames.rs` with Frame struct
4. Create `step.rs` with step() method
5. Create `eval_expr.rs` with eval_expr() and related
6. Create `eval_stmt.rs` with eval_stmt() and related
7. Create `apply.rs` with all apply_* methods
8. Create `mod.rs` that ties everything together
9. Run tests to verify
