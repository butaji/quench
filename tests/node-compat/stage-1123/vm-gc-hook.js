if (typeof globalThis.gc !== "function") throw new Error("gc hook is missing");
if (globalThis.gc() !== undefined) throw new Error("gc hook returned a value");
