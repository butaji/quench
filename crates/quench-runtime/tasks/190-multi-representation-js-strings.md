# 190 — Multi-representation UTF-16 JavaScript strings

Status: planned

Replace `Rc<String>` plus repeated UTF-8 `.chars()` scans with one canonical string sum:
sequential Latin-1, sequential UTF-16, cons/rope, slice, and thin atom. Store UTF-16
code-unit length and a lazy hash-or-array-index cache. Cache single Latin-1 character
strings. Indexing and `charCodeAt` operate on UTF-16 code units as JavaScript requires.

Concatenation creates a cons node only above a named size threshold; consumers needing
contiguous memory call an explicit flatten kernel. Rope depth and retained-parent ratios
are named policies: over-deep ropes flatten, and a small slice of a large parent copies.
All variants implement the same string-view morphism, so RegExp, property atoms, and
string built-ins consume one interface rather than variant branches spread through VM
semantics.

Acceptance: surrogate/lone-surrogate semantics, length/indexing, equality/hash,
concat/slice/substring, RegExp inputs, and atom conversion pass; ASCII `length` and
indexed access are O(1); repeated concatenation avoids quadratic copying; retained-memory
tests enforce the slice policy; full V8v7 A/B improves.

Primary sources: V8 string representations,
<https://chromium.googlesource.com/v8/v8.git/+/refs/heads/main/docs/objects/strings.md>,
and the rope algorithm <https://www.cs.tufts.edu/comp/150FP/archive/hans-boehm/ropes.pdf>.

For append-heavy consumers, build into fixed-size segments and flatten exactly once at
the observing string boundary. This is a builder policy derived from the canonical
string sum, not another string value representation. Specialize consumers separately
for borrowed Latin-1 and UTF-16 views so the common one-byte path does not carry a width
branch per character. V8 reports both patterns in its 2025 stringifier work:
<https://v8.dev/blog/json-stringify>.
