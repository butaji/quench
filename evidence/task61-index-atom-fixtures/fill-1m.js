var values = [];
for (var index = 0; index < 1000000; index++) values[index] = index;
if (values[999999] !== 999999) throw new Error("indexed fill mismatch");
