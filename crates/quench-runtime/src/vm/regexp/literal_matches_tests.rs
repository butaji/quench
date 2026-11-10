use super::*;
use crate::Engine;
use crate::test_host::TestHost;
use std::{cell::RefCell, rc::Rc};

/// A global match of a plain ASCII literal run (`/\n/g`, `/,,/g`) and of every other pattern
/// through `match`, `replace`, `split` and `matchAll` finds Node's matches, leaves `lastIndex`
/// and the legacy `RegExp` statics as Node does, over ASCII and non-ASCII subjects,
/// case-insensitive and sticky patterns, overlapping candidates and no match at all.
#[test]
fn global_regexp_matches_find_literal_and_general_matches() {
    let source = r#"
const o = [];
const t = "a\nbb\n\nccc\n", u = "x,y,,z", w = "é\nü\n\u{1F600}\n";
for (const [s, r] of [[t, /\n/g], [t, /\n\n/g], [t, /b/g], [t, /bb\n/g], [u, /,/g], [u, /,,/g], [w, /\n/g], [t, /B/gi], [t, /q/g], ["aaaa", /aa/g], ["aaaa", /a/gy], ["baaa", /a/gy], ["", /a/g], [t, /\n/gu], [t, /\n/gm], ["a.b.c", /\./g], ["a.b.c", /./g]]) {
  o.push(JSON.stringify(s.match(r)), JSON.stringify(s.replace(r, "<$&>")), JSON.stringify(s.replace(r, "-")), JSON.stringify(s.split(r)), r.lastIndex, JSON.stringify([...s.matchAll(r)].map((m) => m.index)), JSON.stringify(RegExp.lastMatch), RegExp["$`"].length);
}
const big = "line\n".repeat(5000);
o.push(big.match(/\n/g).length, big.substring(0, 12345).match(/\n/g).length, big.replace(/ne\n/g, "").length);
let total = 0;
for (let i = 0; i < 3000; i++) total += t.match(/\n/g).length + u.match(/,/g).length + w.match(/\n/g).length + "aaaa".match(/aa/g).length + (t.match(/B/gi) || []).length;
o.push(total);
print(o.join("|"));
"#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "regexp-global-match.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        [r#"["\n","\n","\n","\n"]|"a<\n>bb<\n><\n>ccc<\n>"|"a-bb--ccc-"|["a","bb","","ccc",""]|0|[1,4,5,9]|"\n"|9|["\n\n"]|"a\nbb<\n\n>ccc\n"|"a\nbb-ccc\n"|["a\nbb","ccc\n"]|0|[4]|"\n\n"|4|["b","b"]|"a\n<b><b>\n\nccc\n"|"a\n--\n\nccc\n"|["a\n","","\n\nccc\n"]|0|[2,3]|"b"|3|["bb\n"]|"a\n<bb\n>\nccc\n"|"a\n-\nccc\n"|["a\n","\nccc\n"]|0|[2]|"bb\n"|2|[",",",",","]|"x<,>y<,><,>z"|"x-y--z"|["x","y","","z"]|0|[1,3,4]|","|4|[",,"]|"x,y<,,>z"|"x,y-z"|["x,y","z"]|0|[3]|",,"|3|["\n","\n","\n"]|"é<\n>ü<\n>😀<\n>"|"é-ü-😀-"|["é","ü","😀",""]|0|[1,3,6]|"\n"|6|["b","b"]|"a\n<b><b>\n\nccc\n"|"a\n--\n\nccc\n"|["a\n","","\n\nccc\n"]|0|[2,3]|"b"|3|null|"a\nbb\n\nccc\n"|"a\nbb\n\nccc\n"|["a\nbb\n\nccc\n"]|0|[]|"b"|3|["aa","aa"]|"<aa><aa>"|"--"|["","",""]|0|[0,2]|"aa"|2|["a","a","a","a"]|"<a><a><a><a>"|"----"|["","","","",""]|0|[0,1,2,3]|"a"|3|null|"baaa"|"baaa"|["b","","",""]|0|[]|"a"|3|null|""|""|[""]|0|[]|"a"|3|["\n","\n","\n","\n"]|"a<\n>bb<\n><\n>ccc<\n>"|"a-bb--ccc-"|["a","bb","","ccc",""]|0|[1,4,5,9]|"\n"|9|["\n","\n","\n","\n"]|"a<\n>bb<\n><\n>ccc<\n>"|"a-bb--ccc-"|["a","bb","","ccc",""]|0|[1,4,5,9]|"\n"|9|[".","."]|"a<.>b<.>c"|"a-b-c"|["a","b","c"]|0|[1,3]|"."|3|["a",".","b",".","c"]|"<a><.><b><.><c>"|"-----"|["","","","","",""]|0|[0,1,2,3,4]|"c"|4|5000|2469|10000|42000"#]
    );
}
