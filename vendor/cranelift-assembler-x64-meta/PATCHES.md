# Local assembler metadata patch

Based on crates.io `cranelift-assembler-x64-meta` 0.134.4, package checksum
`cbf8909326a466739e83ffe11e74c0c8c6b4bfa911b612aece915daa746cff58`.

The sole instruction extension is the 128-bit immediate `vprolq` form in
`src/instructions/shift.rs`. The existing generator owns its operand encoding,
register allocation interface, display and AVX-512F/VL feature requirements.
Quench's Cranelift lowering uses it only for complementary logical shifts of
the same i64 lane value; unsupported targets retain their existing lowering.
