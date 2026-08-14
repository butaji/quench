# Goal: stages 104–107

Bring test262 stages 104–107 to 100% passing through the canonical runner:
ListFormat, Locale, Number, and NumberFormat. Preserve locale-list processing,
constructor/prototype identity, accessor metadata, option property-get order,
formatting and resolvedOptions key order, and exact completion/error behavior.
Do not edit test262 or the harness. Re-run owned stages and earlier regressions
after each fix; finish with clean format/clippy checks and committed verified
changes.
