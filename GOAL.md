# Goal: residual stages 104–106

Stage 107 (NumberFormat) is verified at 249/249. Bring the remaining test262
stages 104–106 to 100% through the canonical runner: ListFormat, Locale, and
Number. Preserve locale-list processing,
constructor/prototype identity, accessor metadata, option property-get order,
formatting and resolvedOptions key order, and exact completion/error behavior.
Do not edit test262 or the harness. Re-run stages 104–107 and earlier
regressions after each fix; finish with clean format/clippy checks and
committed verified changes.
