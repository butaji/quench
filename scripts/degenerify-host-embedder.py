#!/usr/bin/env python3
"""Rewrite an embedder crate for the erased host (TOD-2034): native and collection contexts,
host function tables and native programs name no host type; a native recovers its host with
`host_as::<T>()`. Usage: degenerify-host-embedder.py <host type path | -> <files...>"""
import re, sys
host = sys.argv[1]
rules = [
    (re.compile(r"\b(NativeContext|CollectionContext)<('\w+|'_),\s*[A-Za-z_][\w:]*>"), r"\1<\2>"),
    (re.compile(r"\b(HostFunction|HostCallback|NativeJsProgram|NativeProgramResolver)<[A-Za-z_][\w:]*>"), r"\1"),
    (re.compile(r"\b(c|cx|ctx|context)(\s*)\.host_mut\(\)"), r"\1\2.host_as::<" + host + ">()"),
]
if host == "-":  # types only; leave host_mut calls to the compiler
    rules = rules[:2]
for path in sys.argv[2:]:
    text = open(path).read()
    new = text
    for pattern, replacement in rules:
        new = pattern.sub(replacement, new)
    if new != text:
        open(path, "w").write(new)
        print(path)
