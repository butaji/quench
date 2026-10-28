// A wide function environment, captured per iteration by closures.
var src = "var fns = [];\n"; for (let i = 0; i < 300; i++) src += `var v${i} = ${i};\n`;
src += "for (let i = 0; i < 2000; i++) { let j = i * 2; fns.push(() => i + j + v7 + v299); }\n";
src += "v7 = 1000; var s = 0; for (const f of fns) s += f(); fns[3]();\n";
src += "for (let k = 0; k < 3; k++) { for (let m = 0; m < 2; m++) fns.push(() => k * 10 + m + v1); }\n";
src += "var tail = fns.slice(-6).map(f => f()); var w = []; for (let a = 0, b = 10; a < 3; a++, b--) w.push(() => [a, b]); a = 0;\n";
src += "console.log(s, fns[3](), tail.join(), w.map(f => f().join(':')).join());";
new Function(src)();
