# Node compatibility stages

The Node test runner parses the `### N. Name` headings below. Keep IDs and
names stable: this is runner input, not a completion claim or skip policy.
Compare observable values, errors, ordering, exit status and host effects with
the local Node oracle. See [repository rules](AGENTS.md).

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
