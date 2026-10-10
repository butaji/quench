(function () { const array = [0]; array.value = 0; for (let i = 0; i < 0; i++) array.value = i; if (array.value !== 0) throw new Error("array write zero"); })();
