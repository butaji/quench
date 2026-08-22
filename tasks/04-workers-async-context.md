# Workers and async context

Status: partial.

Synchronous Worker lifecycle validation and AsyncLocalStorage defaults, nesting, bind, snapshot, exit, and disable behavior are implemented; asynchronous worker execution and complete propagation remain.

Acceptance: worker lifecycle/message fixtures and async-context boundary fixtures pass without alternate runtimes.
