function count(input) {
  var expression = /ab+c/g;
  var matches = 0;
  while (expression.exec(input) !== null) {
    matches++;
  }
  return matches;
}

count("abc");
var result = count("xxabc abbc z abbbc no");
if (result !== 3) {
  throw new Error("regexp literal scan mismatch");
}
return result;
