# Fastest single VM, not the current JS shape

The goal is the fastest possible VM for both JavaScript and Wasm. Existing execute-path ADRs (compact JS instruction stream, 8-byte tagged JS value) describe today’s JS engine; they do not limit this design. Typed specialized operations and register slots are in; a universal boxed Value is out.

**Considered Options**: keep ADR 0004/0005 as binding constraints and fit Wasm into the current tagged word.
