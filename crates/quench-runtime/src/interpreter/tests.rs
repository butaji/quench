//! Unit tests for interpreter helpers.

#[allow(unused_imports)]
use crate::interpreter::has_legacy_octal;

#[test]
fn test_reset_depth() {
    crate::interpreter::reset_depth();
}

#[test]
fn test_has_legacy_octal() {
    assert!(has_legacy_octal("01"), "01 is legacy octal");
    assert!(has_legacy_octal("07"), "07 is legacy octal");
    assert!(!has_legacy_octal("0x1"), "0x1 is hex, not octal");
    assert!(!has_legacy_octal("0X1"), "0X1 is hex, not octal");
    assert!(!has_legacy_octal("0b1"), "0b1 is binary, not octal");
    assert!(!has_legacy_octal("0B1"), "0B1 is binary, not octal");
    assert!(!has_legacy_octal("0o1"), "0o1 is octal, not legacy");
    assert!(!has_legacy_octal("0O1"), "0O1 is octal, not legacy");
    assert!(!has_legacy_octal("0n"), "0n is bigint, not octal");
    assert!(has_legacy_octal("a = 01;"), "a = 01; has octal");
    assert!(
        has_legacy_octal("\"use strict\";\na = 01;"),
        "with strict prefix"
    );
    assert!(
        !has_legacy_octal("\"use strict\";\nvar threw = false;"),
        "strict source, no octal"
    );
    // Copyright year must NOT be flagged
    assert!(
        !has_legacy_octal("// Copyright (C) 2015 the V8 project authors."),
        "copyright 2015"
    );
    assert!(
        !has_legacy_octal("// Copyright (C) 2016 the V8 project authors."),
        "copyright 2016"
    );
    // Numbers with embedded 01/07 are not octals
    assert!(!has_legacy_octal("var x = 2015;"), "2015 not octal");
    assert!(!has_legacy_octal("var x = 1007;"), "1007 not octal");
    assert!(!has_legacy_octal("var x = 1.07;"), "1.07 not octal");
    assert!(!has_legacy_octal("var x = 0.07;"), "0.07 not octal");
    // Strings with embedded octals must not be flagged
    assert!(
        !has_legacy_octal(r#"assert.sameValue(decimalToHexString(1), "0001");"#),
        "0001 string"
    );
    assert!(
        !has_legacy_octal(r#"var hex = "0123456789ABCDEF";"#),
        "hex string"
    );
    assert!(
        !has_legacy_octal(r#"assert.sameValue(decimalToPercentHexString(1), "%01");"#),
        "%01 string"
    );
    // Full test source
    assert!(
        !has_legacy_octal(
            r#""use strict";
function decimalToHexString(n) {
  var hex = "0123456789ABCDEF";
  return "%" + hex[(n >> 4) & 0xf] + hex[n & 0xf];
}
assert.sameValue(decimalToHexString(1), "0001");
assert.sameValue(decimalToPercentHexString(1), "%01");"#
        ),
        "decimalToHexString.js"
    );
    // Template literal with embedded 0 — not an octal
    assert!(
        !has_legacy_octal(r#"var s = `prefix 01 suffix`;"#),
        "template literal"
    );
    // Block comment with embedded 0 — not an octal
    assert!(
        !has_legacy_octal(r#"/* octal: 01 in comment */"#),
        "block comment"
    );
    // Regex literal with \u02C1
    assert!(
        !has_legacy_octal(
            r#""use strict";
var UnicodeIDStart = /[a-zA-Z\xF6\xF8-\u02C1]/u;"#
        ),
        "regex with \\u02C1"
    );
    // [native code] matcher
    assert!(
        !has_legacy_octal(
            r#""use strict";
var re = /\[native code\]/"#
        ),
        "native code regex"
    );
    // UTF-8 multi-byte
    assert!(
        !has_legacy_octal("var _\u{0AFA}\u{0AFB}\u{0AFC};"),
        "UTF-8 multi-byte"
    );
    // Regex char class 01
    assert!(
        !has_legacy_octal(r#"var re = /[01]/"#),
        "regex char class 01"
    );
    // Non-octal decimals (08, 09, 018, etc.)
    assert!(!has_legacy_octal("08"), "08 not octal");
    assert!(!has_legacy_octal("09"), "09 not octal");
    assert!(!has_legacy_octal("018"), "018 not octal");
    assert!(!has_legacy_octal("019"), "019 not octal");
    assert!(
        !has_legacy_octal("assert.sameValue(08, 8);"),
        "08 in assert"
    );
    assert!(
        !has_legacy_octal("assert.sameValue(018, 18);"),
        "018 in assert"
    );
    // Numeric separators
    assert!(!has_legacy_octal("var x = 00_01;"), "00_01 not octal");
    assert!(
        !has_legacy_octal("assert.sameValue(10.00_01e2, 10.0001e2);"),
        "10.00_01e2 not octal"
    );
    // Actual octals must be detected
    assert!(has_legacy_octal("var x = 01;"), "01 in code is octal");
    assert!(
        has_legacy_octal("assert.sameValue(01, 1);"),
        "01 in assert is octal"
    );
}
