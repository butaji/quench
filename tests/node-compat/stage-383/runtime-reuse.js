globalThis.__reuseCounter = (globalThis.__reuseCounter || 0) + 1;
if (globalThis.__reuseCounter !== 1) throw new Error("global state leaked");
