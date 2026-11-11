//! Strings concatenated as parts (`vm::wtf16::Parts`) read as the flat text, however they are
//! first used and across collections.
use crate::subset_tests::output;

/// A concatenation held as parts keeps its pieces alive through collections, and reads the
/// same as the flat text as a key, in `===`, as a length, and as a part of another one.
#[test]
fn concatenation_parts_survive_collection_and_read_as_flat_text() {
    let source = r#"
      function make(tag) { return 'String for key ' + tag + ' in leaf node'; }
      function churn() { for (var k = 0; k < 40000; k++) { make(k + 0.25); } }
      var kept = [];
      for (var i = 0; i < 64; i++) { kept.push(make(i + 0.5)); }
      churn();
      var table = {};
      table[kept[3]] = 'found';
      churn();
      print(kept[0]); print(kept[63].length);
      print(table['String for key 3.5 in leaf node']);
      print(kept[7] === 'String for key 7.5 in leaf node');
      var chain = make(1);
      for (var j = 0; j < 4; j++) { chain = make(j) + chain; churn(); }
      print(chain.length);
    "#;
    let expected = ["String for key 0.5 in leaf node", "32", "found", "true", "145"];
    assert_eq!(output(source), expected);
}
