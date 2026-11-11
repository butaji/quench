use crate::vm::Vm;
use crate::Engine;
use crate::test_host::TestHost;
use std::{cell::RefCell, rc::Rc};

/// A pattern run until its program has native code (`FIRST_REQUEST` searches) gives
/// every subject the same `exec` result as its first, interpreted search did: greedy and
/// lazy repeats that give units back, optional groups, alternation, captures and loops.
#[test]
fn native_regexp_search_matches_the_interpreter() {
    let source = r#"
const patterns = [
  /(?:a+:)?(.)/, /(?:a+:)(.)|(.)/, /(((\w+):\/\/)([^\/:]*)(:(\d+))?)?([^#?]*)(\?([^#]*))?(#(.*))?/,
  /^(([^:\/?#]+):)?(\/\/([^\/?#]*))?([^?#]*)(\?([^#]*))?(#(.*))?$/, /(a+?)(b*)c/, /(\d{1,3})(?:\.(\d{1,3}))*/,
  /(?:(x)|y)+z/, /(a|ab)(c|bcd)(d*)/, /\b(\w+)\s+\1\b|\w+/, /[a-c]+?[b-d]{2,}/, /^(?:\s*(\w+)\s*=\s*([^;]*);?)+$/,
  /(a*)*b/, /(?:a*?)*?c/, /(.*?),(.*)/, /x*y+z?/,
];
const subjects = ["aaab", "aaa:b", "abcd/x", "uggc:/jjj.snprobbx.pbz/ybtva.cuc", "http://host:80/p?q#h", "a=1; b=2;", "abbbc", "aaac",
  "10.20.30", "xyxz", "abcd", "foo foo bar", "bcbd", "", "x,y,z", "xxyyz"];
const out = (re, s) => { re.lastIndex = 0; const m = re.exec(s); return m === null ? "null" : m.index + ":" + JSON.stringify([...m]); };
const bad = [];
for (const re of patterns) {
  const first = subjects.map((s) => out(re, s));
  for (let i = 0; i < 2100; i++) re.exec(subjects[i % subjects.length]);
  const after = subjects.map((s) => out(re, s));
  after.forEach((r, k) => { if (r !== first[k]) bad.push(String(re) + " on " + JSON.stringify(subjects[k]) + ": " + first[k] + " vs " + r); });
}
print(bad.join("\n") + "|" + bad.length);
"#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    let program = Engine::specialize(source, "regexp-native.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["|0"]);
}
