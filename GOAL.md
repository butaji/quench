# Goal: stages 108–113

Bring test262 stages 108–113 to 100% passing through the canonical runner:
PluralRules, RelativeTimeFormat, Segmenter, String, Temporal, and TypedArray.
Preserve ECMA-402 option validation and getter order, locale behavior,
formatting, prototype/accessor descriptors, completion/error ordering, and
typed-array integration. Fix shared canonical semantics; never alter the
harness or test262. Re-run owned stages and earlier regressions after each
fix, finish quality checks, and commit verified changes.
