# Splay compact-cell layout probe

This probe checks whether the proposed Object/Array payload dimensions fit a
safe Rust tagged enum on the current M4 toolchain. It uses machine-word
stand-ins for `Value` and the storage metadata; `Rc<Vec<u64>>` matches the
pointer-sized `Rc<Vec<Value>>` representation. The first payload word models
the packed prototype/shape identity; the cold enum is deliberately larger,
then boxed behind the common `Cell` enum.

Command:

```sh
rustc cell-layout-probe.rs -o /tmp/task61-cell-layout-probe
/tmp/task61-cell-layout-probe
```

Host compiler: Rust 1.99.0, `aarch64-apple-darwin`, LLVM 23.1.2.

Output:

```text
object_data=24 array_data=24 cold_enum=104 cell=32 option_cell=32 slot=32 rc_vec=8
```

This proves only that these payload dimensions can fit in a 32-byte Rust enum
and `Option` on this compiler. It does not measure Quench's runtime layout,
allocator overhead, RSS, speed, or correctness. The production representation
still needs compile-time size assertions and the full Splay plus all-eight
gates.
