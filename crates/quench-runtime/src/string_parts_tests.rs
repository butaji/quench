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

/// Small strings (a Number with at most one string on each side) read as their text whichever
/// side they have, as parts of other strings, as keys and through the string natives, before and
/// after collections, whether first read before or after one.
#[test]
fn small_strings_read_as_flat_text_across_collections() {
    let source = r#"
      function pre(n) { return 'a fairly long prefix text ' + n; }
      function post(n) { return n + ' and a fairly long suffix text'; }
      function both(n) { return 'String for key ' + n + ' in leaf node'; }
      function churn() { var t = []; for (var k = 0; k < 30000; k++) { t.push(both(k)); if (t.length > 500) t = []; } }
      var unread = [], read = [];
      for (var i = 0; i < 300; i++) { unread.push(both(i)); read.push(post(i)); }
      for (var i = 0; i < 300; i++) { if (read[i].length < 10) print('short'); }
      churn();
      var nested = 'x-prefix-text ' + 7 + ' ' + both(9);
      var inner = both(1.5) + 3 + ' and then the rest';
      churn();
      print(unread[299]); print(read[0]); print(pre(-0.5));
      print(nested); print(inner);
      var keyed = {}; keyed[both(42)] = 'k'; churn();
      print(keyed['String for key 42 in leaf node'] + ' ' + keyed[both(42)]);
      var map = new Map(); map.set(both(5), 1); churn();
      print(map.get('String for key 5 in leaf node'));
      print(unread[10].charCodeAt(15) + ' ' + unread[10].charAt(16) + ' ' + unread[11].length);
      print((unread[12] === both(12)) + ' ' + (unread[13] < unread[14]) + ' ' + JSON.stringify(unread[1]));
      print(unread.join('|').length);
      var m = ' middle words here ', w = 'the tail end', firsts = [], lasts = [];
      for (var i = 0; i < 200; i++) { firsts.push(i + m + w); lasts.push(w + m + i); }
      churn();
      print(firsts[199] + '|' + lasts[198]);
    "#;
    let mut joined = 0;
    for i in 0..300 {
        joined += format!("String for key {i} in leaf node").len() + 1;
    }
    let expected = [
        "String for key 299 in leaf node".to_string(),
        "0 and a fairly long suffix text".into(),
        "a fairly long prefix text -0.5".into(),
        "x-prefix-text 7 String for key 9 in leaf node".into(),
        "String for key 1.5 in leaf node3 and then the rest".into(),
        "k k".into(),
        "1".into(),
        "49 0 30".into(),
        "true true \"String for key 1 in leaf node\"".into(),
        (joined - 1).to_string(),
        "199 middle words here the tail end|the tail end middle words here 198".into(),
    ];
    assert_eq!(output(source), expected);
}

/// More small strings read once than the views kept between collections (`SmallViews`): each
/// read answers its own text, through the in-place readers and through views alike.
#[test]
fn many_small_strings_read_once_answer_their_own_text() {
    let source = r#"
      var a = [];
      for (var i = 0; i < 100000; i++) a.push('String for key ' + i + ' in leaf node');
      var bad = 0, total = 0;
      for (var i = 0; i < a.length; i++) {
        var s = a[i];
        if (s.indexOf(' ' + i + ' ') !== 14) bad++;
        if (s.slice(15, 15 + String(i).length) !== String(i)) bad++;
        total += s.length + s.charCodeAt(15);
      }
      var joined = a.join(',');
      print(bad); print(total); print(joined.length); print(a[99999].toUpperCase());
    "#;
    let (mut total, mut joined) = (0, 0);
    for i in 0..100000 {
        let text = format!("String for key {i} in leaf node");
        total += text.len() + usize::from(text.as_bytes()[15]);
        joined += text.len() + 1;
    }
    let expected = ["0".to_string(), total.to_string(), (joined - 1).to_string(), "STRING FOR KEY 99999 IN LEAF NODE".into()];
    assert_eq!(output(source), expected);
}
