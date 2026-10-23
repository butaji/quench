const out = [];
function rope(a, b) { return a + b; }
const big = "x".repeat(600);
const r1 = rope(big, "-abc-def-ABC-" + "y".repeat(600));
const r2 = rope("é".repeat(520), "-abc-ü-" + "z".repeat(530));
for (const s of [r1, r2]) {
  out.push(s.replace(/abc/, "[$&]").length, s.replace(/abc/gi, "<$`|$'>").length);
  out.push(s.replace(/(a)(b)(c)/g, "$3$2$1$$").slice(590, 640));
  out.push(s.replace(/(?<one>a)bc/g, "$<one>!").slice(595, 640));
  out.push(s.replace(/-/g, (m, i) => "#" + i).slice(590, 660));
  const st = /abc/y; st.lastIndex = 0; out.push(s.replace(st, "S").length);
  out.push(s.replace(/q/g, "Q") === s, s.replace(/abc/g, "lit").length);
  out.push(RegExp.lastMatch, RegExp.leftContext.length, RegExp.rightContext.length, RegExp.input.length);
  out.push(s.length, s.indexOf("abc"), s.charCodeAt(600));
}
console.log(JSON.stringify(out));
