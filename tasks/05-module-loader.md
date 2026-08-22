# Module loader compatibility

Status: partial.

Implemented and tested canonical CommonJS `module` loading with `builtinModules`; ESM APIs and full `createRequire` integration remain.
Expand `node:module` beyond CommonJS require with one real, tested loader or ESM API at a time. Preserve OXC syntax ownership and the existing centralized resolver.
