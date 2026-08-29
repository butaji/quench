# Compatibility stages

A stage is a semantic planning boundary, not a progress claim or fixture
workaround. Use local Node tests and CLI as oracle; declare reusable API facts
once, generate mechanics from them, and handwrite only observable algorithms
and host adapters. `Proven`, `Guarded`, and `Unknown` are distinct: unknown
behavior uses complete semantics or fails visibly.

Claim completion only from a clean, reproducible verification with no
unexplained failure or timeout. Compare values, descriptors, identity,
ordering, errors, exit status, and host effects. The VM remains the only JS
semantics; the host never recognizes benchmark or fixture identity.

## Families

1. Measurement and runner truth
2. Runtime and value semantics
3. Core data APIs
4. Events, scheduling, and async context
5. Filesystem, VFS, and modules
6. Streams and backpressure
7. Network and protocols
8. Process, workers, and clusters
9. Crypto, security, and policy
10. Observability and performance APIs
11. Web and special modules
12. Full-suite closure and reduction

## Execution stages

### 0. Measurement and runner truth
### 1. Runtime and value semantics
### 2. Core data APIs
### 3. Events, scheduling, and async context
### 4. Filesystem, VFS, and modules
### 5. Streams and backpressure
### 6. Network and protocols
### 7. Process, workers, and clusters
### 8. Crypto, security, and policy
### 9. Observability and performance APIs
### 10. Web and special modules
### 11. Full-suite closure and reduction
