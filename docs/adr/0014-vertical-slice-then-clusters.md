# Vertical slice, then spec-dependency clusters

Implement and verify the spec suite in dependency order. The harness must score
each directive, keep validator and execution outcomes distinct, and report
observable failures without relying on a file-level skip plan.
