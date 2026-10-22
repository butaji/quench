(function () { const array = [3]; array.value = 3; let result = 0; for (let i = 0; i < 0; i++) result = array.value; if (result !== 0) throw new Error("array read zero"); })();
