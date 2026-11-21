#!/usr/bin/env python3
"""Drop the `H: Host` type parameter from quench-runtime (TOD-2034).

Rewrites generic lists and type arguments that name the host parameter `H`:
`<H: Host>` and `, H: Host` go, `Vm<H>` / `Foo<'a, H>` / `::<H, X>` lose `H`.
Run from the repo root; other branches can run it to rebase onto the change.
Files whose `H` is another parameter (Hasher, Into) only name it in `<H: Bound>`
lists this script does not touch.
"""
import pathlib, re, sys

ROOT = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else "crates/quench-runtime/src")
HOST = r"(?:(?:crate|super|super::super)::(?:host::)?)?Host"
rules = [
    # generic parameter lists
    (re.compile(r"<H: " + HOST + r">"), ""),
    (re.compile(r"<H: " + HOST + r",\s*"), "<"),
    (re.compile(r",\s*H: " + HOST + r"(?=\s*[,>])"), ""),
    # `impl<> X` left by the rules above
    (re.compile(r"\bimpl<>"), "impl"),
    # type arguments
    (re.compile(r"::<H>"), ""),
    (re.compile(r"::<H,\s*"), "::<"),
    (re.compile(r"(?<=[A-Za-z0-9_])<H>"), ""),
    (re.compile(r"(?<=[A-Za-z0-9_])<H,\s*"), "<"),
    (re.compile(r",\s*H>"), ">"),
    (re.compile(r",\s*H(?=,)"), ""),
]
changed = 0
for path in ROOT.rglob("*.rs"):
    text = path.read_text()
    new = text
    for pattern, replacement in rules:
        new = pattern.sub(replacement, new)
    if new != text:
        path.write_text(new)
        changed += 1
print(f"rewrote {changed} files")
