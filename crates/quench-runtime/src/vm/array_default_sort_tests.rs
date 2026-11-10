//! Default `sort` order, replacement templates and rest parameters against Node's output.

use crate::test_host::printed;

/// Without a comparator, values follow their strings' UTF-16 code units (an astral pair's
/// lead surrogate before U+FF5E, a lone surrogate by its unit), `undefined` and holes go last,
/// an object's `toString` runs at comparisons, and a Symbol throws only once compared.
#[test]
fn default_sort_orders_by_code_units() {
    let source = r#"
let calls = 0; const obj = (s) => ({ toString() { calls++; return s; } });
print(JSON.stringify(["\u{1F600}", "\uFF5E", "a", 10, 9, 1, null, true, false, -0, NaN, 1e21, "B", "", "ab", "\ud800", "\udfff", "\uffff", 3n, -2n].sort(), (k, v) => typeof v === "bigint" ? v + "n" : v));
const holes = [3, 20, 100, undefined, , 5, , null];
holes.sort();
print(holes.length + ":" + Object.keys(holes).join(",") + ":" + holes.map((v) => v === undefined ? "u" : String(v)).join("|"));
const objects = [obj("b"), obj("a"), obj("c"), "bb", 1];
print(objects.sort().join(",") + ":" + (calls >= 2 * (objects.length - 1)));
for (const values of [[Symbol("x"), 1], [1, Symbol("x")], [Symbol(), Symbol()]]) {
  try { values.sort(); print("no throw"); } catch (e) { print(e.constructor.name); }
}
print([Symbol.iterator].sort().length + ":" + [].sort().length + ":" + [undefined, Symbol()].sort().length);
const source = ["b", "a", 10, 9];
print(source.toSorted().join() + ":" + source.join() + ":" + [, "b", undefined, "a"].toSorted().map(String).join());
print([3n, 20n, 100n, -1n].sort().join() + ":" + [2, 1, 10].sort().join());
"#;
    assert_eq!(
        printed(source, "default-sort.js"),
        [
            r#"["","-2n",0,1,10,1e+21,"3n",9,"B",null,"a","ab",false,null,true,"\ud800","😀","\udfff","～","￿"]"#,
            "8:0,1,2,3,4,5:100|20|3|5|null|u||",
            "1,a,b,bb,c:true",
            "TypeError",
            "TypeError",
            "TypeError",
            "1:0:2",
            "10,9,a,b:b,a,10,9:a,b,undefined,undefined",
            "-1,100,20,3:1,10,2",
        ]
    );
}

/// GetSubstitution: `$$`, `$&`, `` $` ``, `$'`, `$n`/`$nn` within the capture count, `$<name>`
/// with named groups, and every other `$` (trailing, before a non-digit, an absent group)
/// kept as text, through `replace` and `replaceAll`.
#[test]
fn replacement_templates_substitute_like_node() {
    let source = r#"
for (const [s, pat, rep] of [["a-b-c", "-", "+"], ["a-b-c", /-/g, "++"], ["abc", "b", "$&$&"], ["abc", "b", "x$$y$"], ["abc", /(b)/, "[$1$2$`$']"], ["abc", "b", "$"], ["abc", "b", ""], ["aXbXc", /x/gi, "é$"], ["abc", /(?<n>b)/, "<$<n>>$<m>x$"], ["abc", "z", "q"], ["a.b", ".", "$'$`"], ["abcabc", /b/g, "12$345"], ["abab", "b", "$`|$'|$&"], ["aaa", "a", "$$$"], ["abc", /(a)(b)(c)/, "$3$2$1$0$4$03$10"], ["xyx", "x", "-$-$a-"]]) print(s.replace(pat, rep) + " " + (typeof pat === "string" ? s.replaceAll(pat, rep) : "-"));
"#;
    assert_eq!(
        printed(source, "replace-templates.js"),
        [
            "a+b-c a+b+c",
            "a++b++c -",
            "abbc abbc",
            "ax$y$c ax$y$c",
            "a[b$2ac]c -",
            "a$c a$c",
            "ac ac",
            "aé$bé$c -",
            "a<b>x$c -",
            "abc abc",
            "abab abab",
            "a12$345ca12$345c -",
            "aa|ab|bab aa|ab|baaba||b",
            "$$aa $$$$$$",
            "cba$0$4ca0 -",
            "-$-$a-yx -$-$a-y-$-$a-",
        ]
    );
}

/// A rest parameter is a fresh, ordinary array of the arguments past the named ones, empty
/// when there are none, and not counted in the function's `length`.
#[test]
fn rest_parameters_collect_the_remaining_arguments() {
    let source = r#"
function r(a, ...rest) { rest.push(a); return rest; }
function r2(...rest) { rest[5] = 1; return rest.length + ":" + Array.isArray(rest) + ":" + rest.join(); }
function r3(a, ...rest) { return rest.length + ":" + r3.length + ":" + JSON.stringify(rest); }
print([JSON.stringify(r(1)), JSON.stringify(r(1, 2, 3)), JSON.stringify(r()), r2(), r2(1, 2), r(1, 2).length, Object.getPrototypeOf(r(1, 2)) === Array.prototype].join(" "));
print([r3(), r3(1), r3(1, 2), r3(1, 2, 3, 4, 5), r3(1, undefined)].join(" "));
const a = r3.bind(null, 0); print(a(7, 8));
let first = null; function keep(...rest) { if (!first) first = rest; return rest; } keep(1); print(keep(2) !== first ? "fresh " + JSON.stringify(first) : "shared");
"#;
    assert_eq!(
        printed(source, "rest-parameters.js"),
        [
            "[1] [2,3,1] [null] 6:true:,,,,,1 6:true:1,2,,,,1 2 true",
            "0:1:[] 0:1:[] 1:1:[2] 4:1:[2,3,4,5] 1:1:[null]",
            "2:1:[7,8]",
            "fresh [1]",
        ]
    );
}
