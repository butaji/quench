//! Typed-array index sites that read and stored one kind meet detach, resize and other receivers.
use crate::subset_tests::output;

#[test]
fn kind_specialized_typed_sites_follow_detach_resize_and_receiver_changes() {
    // Sites that read and stored one kind (`typed_sites`) meet a detached buffer, a buffer
    // resized under length-tracking and fixed views, and receivers of other kinds, as node does.
    let source = r#"const out = [];
const rd = (a, k) => a[k];
const wr = (a, k, v) => { a[k] = v; };
// Warm one read site and one store site per kind, then detach, resize and change receiver kind.
for (const Kind of [Uint8Array, Int8Array, Uint8ClampedArray, Uint16Array, Int16Array, Uint32Array, Int32Array, Float32Array, Float64Array]) {
  const a = new Kind(4);
  for (let round = 0; round < 4; round++) for (let i = 0; i < 6; i++) { wr(a, i, i * 70000 - 3); rd(a, i); }
  const warm = [];
  for (let i = -1; i < 6; i++) warm.push(rd(a, i));
  out.push(Kind.name + ":" + warm.join());
  a.buffer.transfer();
  out.push(rd(a, 0) + "," + rd(a, 3));
  wr(a, 0, 5);
  out.push(a.length);
  const buffer = new ArrayBuffer(4 * Kind.BYTES_PER_ELEMENT, { maxByteLength: 8 * Kind.BYTES_PER_ELEMENT });
  const tracking = new Kind(buffer);
  const fixed = new Kind(buffer, 0, 4);
  for (let round = 0; round < 4; round++) for (let i = 0; i < 4; i++) { wr(tracking, i, i + 1); rd(tracking, i); wr(fixed, i, i + 1); rd(fixed, i); }
  const seen = [];
  for (const size of [8, 2, 0, 6, 4]) {
    buffer.resize(size * Kind.BYTES_PER_ELEMENT);
    wr(tracking, 5, 9); wr(fixed, 1, 7);
    seen.push([rd(tracking, 1), rd(tracking, 5), rd(fixed, 1), rd(fixed, 3)].join("/"));
  }
  out.push(seen.join(" "));
  const kinds = [new Kind(2), new Float64Array(2), new Int8Array(2), [4, 5], "xy"];
  for (const k of kinds) { try { wr(k, 1, 3.5); } catch (e) { out.push("E"); } out.push(String(rd(k, 1))); }
}
print(out.join("|"));
"#;
    assert_eq!(output(source), [r#"Uint8Array:,253,109,221,77,,|undefined,undefined|0|7/9/7/4 7/// /// 7/9/7/0 7//7/0|3|3.5|3|3.5|y|Int8Array:,-3,109,-35,77,,|undefined,undefined|0|7/9/7/4 7/// /// 7/9/7/0 7//7/0|3|3.5|3|3.5|y|Uint8ClampedArray:,0,255,255,255,,|undefined,undefined|0|7/9/7/4 7/// /// 7/9/7/0 7//7/0|4|3.5|3|3.5|y|Uint16Array:,65533,4461,8925,13389,,|undefined,undefined|0|7/9/7/4 7/// /// 7/9/7/0 7//7/0|3|3.5|3|3.5|y|Int16Array:,-3,4461,8925,13389,,|undefined,undefined|0|7/9/7/4 7/// /// 7/9/7/0 7//7/0|3|3.5|3|3.5|y|Uint32Array:,4294967293,69997,139997,209997,,|undefined,undefined|0|7/9/7/4 7/// /// 7/9/7/0 7//7/0|3|3.5|3|3.5|y|Int32Array:,-3,69997,139997,209997,,|undefined,undefined|0|7/9/7/4 7/// /// 7/9/7/0 7//7/0|3|3.5|3|3.5|y|Float32Array:,-3,69997,139997,209997,,|undefined,undefined|0|7/9/7/4 7/// /// 7/9/7/0 7//7/0|3.5|3.5|3|3.5|y|Float64Array:,-3,69997,139997,209997,,|undefined,undefined|0|7/9/7/4 7/// /// 7/9/7/0 7//7/0|3.5|3.5|3|3.5|y"#.to_string()]);
}
