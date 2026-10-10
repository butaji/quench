(function () { const object = { value: 0 }; for (let i = 0; i < 0; i++) object.value = i; if (object.value !== 0) throw new Error("write zero"); })();
