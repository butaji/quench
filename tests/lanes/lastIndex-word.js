const regexp = /x/g;
let matches = 0;
for (let i = 0; i < 25000000; i++) {
  regexp.lastIndex = 0;
  if (regexp.exec("x") !== null) matches++;
}
if (matches !== 25000000) throw new Error("lastIndex result");
