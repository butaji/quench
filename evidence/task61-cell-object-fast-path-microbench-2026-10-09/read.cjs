(function () {
  const object = { value: 3 };
  let result = 0;
  for (let i = 0; i < 2000000; i++) result = object.value;
  if (result !== 3) throw new Error("object read");
})();
