(function () {
  let result = 0;
  for (let i = 0; i < 2000000; i++) result = i;
  if (result !== 1999999) throw new Error("local control");
})();
