(function () { const object = { value: 3 }; let result = 0; for (let i = 0; i < 0; i++) result = object.value; if (result !== 0) throw new Error("read zero"); })();
