# 208 — ARM64 Top-Byte-Ignore pointer tagging for the tagged Value

Status: planned

[[55-architecture-target-matrix]] already scopes native stencil execution to AArch64
only; this task is an architecture-specific optimization that is only sound because of
that scoping and would need its own guard if [[56-x86-64-sysv-backend]] ever lands.
AArch64 defines Top-Byte-Ignore (TBI): when enabled, the processor's address-translation
hardware ignores the top 8 bits of a 64-bit virtual address on every load/store and
branch, so a pointer with an arbitrary tag byte in bits 56-63 dereferences correctly
*without* software masking it off first. Android (since API 30, on TBI-capable kernels)
and HWASan/MTE both rely on exactly this to carry a tag alongside every heap pointer for
free.

[[06-word-sized-tagged-value]] presumably already encodes `Value`'s tag somewhere in its
word (NaN-boxing or an equivalent scheme, per the task list's framing relative to
QuickJS's `JS_NAN_BOXING`). The candidate improvement: for the pointer-carrying tag
states specifically (object, string, closure — whichever of `Value`'s variants already
carry a heap pointer), move that state's tag into the pointer's own top byte via TBI
instead of consuming bit patterns from the NaN-boxing space. This has two possible
payoffs to measure, not assume:
1. A pointer dereference on a TBI-tagged pointer needs no explicit `AND`/mask
   instruction before use — the hardware does it. Every property-slot load through a
   guarded IC connector ([[08]], [[129]]) that currently masks a tagged pointer before
   dereferencing could drop that instruction.
2. Freeing pointer-variant tag bits out of the NaN-boxing space may relax pressure on
   [[06]]'s value-representation bit budget, relevant to [[165-typed-property-field-representations]]'s
   representation-widening work.

This requires two verified preconditions before it is anything but a correctness bug:
TBI must actually be enabled for the process (it is opt-in per-platform: Linux via
`PR_SET_TAGGED_ADDR_CTRL`/`PR_TAGGED_ADDR_ENABLE`, and behavior differs on macOS/Apple
Silicon — verify what this VM's actual target platforms from [[55]] guarantee before
relying on it), and every raw pointer comparison/hashing path in the codebase (shape
hash-consing in [[135]], `Rc::ptr_eq`-based sharing in [[16]]/[[42]]) must either mask
the tag byte before comparing or treat differently-tagged pointers to the same object as
intentionally distinct, not accidentally break identity.

Acceptance: TBI is confirmed enabled (or explicitly enabled by this VM) on every
supported AArch64 target from [[55]], verified by a runtime check, not an assumption;
pointer-variant `Value` tags move into the top byte and dereference without an explicit
mask instruction, verified by disassembling a linked stencil that loads through a
tagged-pointer `Value`; every pointer-identity comparison in the codebase is audited and
either masks the tag or is confirmed correct with it; alternating A/B on the full V8v7
suite shows no regression and documents any measured gain; falls back to the existing
NaN-boxing-only tagging scheme cleanly if TBI is unavailable at runtime.

## Platform gate from round-seven research

Do not start implementation on the current macOS target yet. The research found an
authoritative Linux userspace ABI, including explicit restrictions on passing tagged
addresses across kernel interfaces, but no equivalent public Apple guarantee covering
this VM's ordinary allocations, signal/debug paths, and JIT mappings. TBI being an ARM
architectural capability does not establish an operating-system ABI. Before any `Value`
change, produce either an Apple platform source defining the contract or a strictly
contained runtime experiment whose failure falls back before a tagged value is
published. This task remains lower priority than removing the measured Rust helper,
call-frame, and ownership boundaries.

Primary sources:
- ARM AArch64 pointer authentication, BTI, and MTE guide:
  <https://developer.arm.com/-/media/Arm%20Developer%20Community/PDF/Learn%20the%20Architecture/Providing%20protection%20for%20complex%20software.pdf>
- Linux kernel tagged-address ABI:
  <https://www.kernel.org/doc/html/latest/arch/arm64/tagged-pointers.html>
- Linux kernel MTE documentation, which states that MTE builds on TBI:
  <https://www.kernel.org/doc/html/latest/arch/arm64/memory-tagging-extension.html>
- Android tagged-pointer platform contract:
  <https://source.android.com/docs/security/test/tagged-pointers>
