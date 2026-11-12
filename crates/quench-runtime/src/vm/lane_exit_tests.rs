//! Operations the lane answers in place that once left it: each runs often enough for the
//! in-place path, and its output is what Node prints for the same source.
use super::dispatch_fast::super_chain::tests::printed;

/// A number, boolean, bigint or symbol read of a name its prototype chain lacks is `undefined`;
/// a prototype that gains the name, a getter that sees a primitive `this`, or a changed chain
/// are seen at once.
#[test]
fn primitive_field_reads_follow_their_prototypes() {
    let source = r#"
const vals = [1.5, 0, true, false, 10n, Symbol("q"), "str", 5];
function f(p) { return p.isColor ? 1 : 0; }
log(rep(() => vals.map(f).join()));
Number.prototype.isColor = "num";
log(vals.map(f).join());
delete Number.prototype.isColor;
Object.defineProperty(Number.prototype, "isColor", { get() { "use strict"; return typeof this; }, configurable: true });
log(vals.map(f).join());
delete Number.prototype.isColor;
let calls = 0;
for (let i = 0; i < 4000; i++) { if (i === 2000) Boolean.prototype.isColor = 1; calls += f(true); }
log(calls);
delete Boolean.prototype.isColor;
const toString = Number.prototype.toString; delete Number.prototype.toString;
function t(p) { return typeof p.toString; }
log(rep(() => t(1.5), 50));
Number.prototype.toString = toString;
try { rep(() => f(null), 5); } catch (e) { log(e.constructor.name); }
print(out.join(" | "));
"#;
    assert_eq!(
        printed(source, "primitive-fields.js"),
        ["0,0,0,0,0,0,0,0 | 1,1,0,0,0,0,0,1 | 1,1,0,0,0,0,0,1 | 2000 | function | TypeError"]
    );
}
