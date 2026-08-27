# Node compatibility fixtures

These small fixtures exercise the Node API surface implemented by
`quench-node`. Each fixture is a self-contained JavaScript program that exits
successfully only when its observable assertions pass.

Fixtures must remain independent of the test runner, avoid benchmark-specific
behavior, and be checked against the local Node oracle when behavior is added
or changed.
