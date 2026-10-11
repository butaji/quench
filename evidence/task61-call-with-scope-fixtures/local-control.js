function bench() {
  var result = 0;
  for (var i = 0; i < 2000000; i++) result = i;
  return result;
}
print(bench());
