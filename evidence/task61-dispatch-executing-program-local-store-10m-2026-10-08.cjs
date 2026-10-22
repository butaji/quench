function loop() {
  var value = 0;
  for (var index = 0; index < 10000000; index++) value = index;
  if (value !== 9999999) throw new Error('bad local-store loop');
}
loop();
