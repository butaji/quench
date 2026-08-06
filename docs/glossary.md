# Glossary

## VM

The ECMAScript execution engine: parser integration, compiler/interpreter,
heap, garbage collector, and runtime operations. It excludes Node.js host APIs
and module-resolution policy.

## Host API

The small, versioned Rust crate interface through which an embedding host such
as `quench-node` creates realms, registers modules and host functions, holds
opaque rooted values, drives jobs, and reads metrics. It is not a C FFI ABI.

## Isolate

An independently owned JavaScript execution environment with a private heap,
collector, job queue, and OS thread. Values and handles never cross isolates;
communication is explicit and serialized by the host.

## Type fact

Compiler-side information recovered from TypeScript syntax, declaration files,
or configured analysis of JavaScript. It is not a JavaScript runtime value and
cannot change ECMAScript semantics.

## Guard

A runtime check that validates an optimization assumption before specialized
code relies on it.

## Generic path

The fully ECMAScript-conformant implementation used when specialization is not
applicable or a guard fails.

## Deoptimization

Transfer from specialized execution to the generic path while preserving the
program's observable state and completion behavior.
