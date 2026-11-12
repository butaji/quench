//! The feature table: each entry is a JavaScript template plus the integer parameters it draws.
//! `@K` is the block index; `@A`, `@B`, `@C` are the drawn parameters. A template defines
//! `function f@K(i)`, which the driver calls once per hot-loop iteration and digests.

/// An inclusive range a template token draws from.
pub(crate) struct Param {
    pub token: &'static str,
    pub low: u64,
    pub high: u64,
}

pub(crate) struct Feature {
    pub name: &'static str,
    pub params: &'static [Param],
    pub template: &'static str,
}

const fn param(token: &'static str, low: u64, high: u64) -> Param {
    Param { token, low, high }
}

pub(crate) const FEATURES: &[Feature] = &[
    Feature {
        name: "numeric-loop",
        params: &[param("@A", 4, 40), param("@B", 1, 9999), param("@C", 0, 7)],
        template: "function f@K(i) { let s = 0; for (let j = 0; j < @A; j++) { s = (s + ((i + j) * @B ^ (j >> @C))) | 0; } return s; }",
    },
    Feature {
        name: "int32-overflow",
        params: &[param("@A", 2, 97), param("@B", 1, 2147483647), param("@C", 0, 31)],
        template: "function f@K(i) { const a = 2147483647 - (i % @A), b = @B + i; const n = -(-2147483648 + (i % 2)); return ((a + b) | 0) ^ Math.imul(a, b) ^ ((a + b) >>> @C) ^ (n | 0) ^ ((a * b) | 0) ^ (Object.is(0 * -(i % 2 ? 1 : -1), -0) ? 1 : 0); }",
    },
    Feature {
        name: "doubles",
        params: &[param("@A", 3, 30), param("@B", 2, 13)],
        template: "function f@K(i) { let s = 0.5; for (let j = 1; j <= @A; j++) { s += (i + j) / @B + (j % 3 === 0 ? -0.25 : 0.125) * j; s = s % 1000.5; } return s + (i % 5 === 0 ? 1 / 0 - 1 / 0 : 0) + (i % 7 === 0 ? -0 : 0.1 + 0.2); }",
    },
    Feature {
        name: "shape-change-mid-loop",
        params: &[param("@A", 2, 6)],
        template: "function f@K(i) { const o = { a: i, b: 2 }; let s = 0; for (let j = 0; j < 12; j++) { if (j === @A) o.c = j; if (j === @A + 3) delete o.a; s = (s + (o.a | 0) + (o.b | 0) + (o.c | 0)) | 0; o.b = j; } return s; }",
    },
    Feature {
        name: "shape-change-across-calls",
        params: &[param("@A", 20, 400)],
        template: "const g@K = { x: 1, y: 2 }; function f@K(i) { if (i === @A) g@K.z = 5; if (i === @A * 2) Object.defineProperty(g@K, 'x', { get() { return 7; } }); return g@K.x + g@K.y + (g@K.z | 0); }",
    },
    Feature {
        name: "prototype-change",
        params: &[param("@A", 20, 400), param("@B", 1, 50)],
        template: "function P@K(v) { this.v = v; } P@K.prototype.m = function (x) { return this.v + x; }; const p@K = new P@K(@B); function f@K(i) { if (i === @A) P@K.prototype.m = function (x) { return this.v - x; }; if (i === @A * 2) p@K.m = function () { return 0; }; return p@K.m(i); }",
    },
    Feature {
        name: "polymorphic-site",
        params: &[param("@A", 10, 300), param("@B", 2, 97)],
        template: "const sh@K = [{ k: 1, v: 2 }, { v: 3, k: 4 }, { k: 5, w: 1, v: 6 }, [7, 8], 'str', 9]; function f@K(i) { if (i % @B === 0) sh@K[0].k = i; const o = sh@K[i % (i < @A ? 2 : 6)]; return ((o.k || 0) + (o.v || 0) + (o.length || 0)) | 0; }",
    },
    Feature {
        name: "calls",
        params: &[param("@A", 1, 99), param("@B", 1, 9), param("@C", 6, 14)],
        template: "function sq@K(x) { return x * x; } function fib@K(n) { return n < 2 ? n : fib@K(n - 1) + fib@K(n - 2); } const obj@K = { base: @A, m(x) { return this.base + x; } }; function mk@K(step) { let n = 0; return () => (n += step); } const c@K = mk@K(@B); class K@K { constructor(v) { this.v = v; } get() { return this.v; } } class L@K extends K@K { get() { return super.get() + 1; } } function f@K(i) { return sq@K(i & 255) + fib@K(i % @C) + obj@K.m(i) + c@K() + new L@K(i).get() + [1, 2, 3].map((x) => x + i).length; }",
    },
    Feature {
        name: "exceptions",
        params: &[param("@A", 2, 50), param("@B", 2, 50)],
        template: "class E@K extends Error { constructor(m, c) { super(m); this.code = c; } } function thrower@K(i) { if (i % @A === 0) throw new E@K('boom' + i, i); if (i % @B === 1) null.x; return i; } function f@K(i) { let s = 0; try { s += thrower@K(i); } catch (e) { s += e instanceof E@K ? e.code : e.name.length; } finally { s += 1; } return s; }",
    },
    Feature {
        name: "array-holes",
        params: &[param("@A", 6, 10)],
        template: "function f@K(i) { const a = [1, , 3, , 5]; a[@A + (i % 3)] = i; delete a[0]; let s = a.length; for (let j = 0; j < a.length; j++) { s = j in a ? (s + a[j]) | 0 : s ^ 1; } a.forEach((x) => { s = (s + x) | 0; }); return s + a.join('-').length + (a[1] === undefined ? 1 : 0) + Object.keys(a).length; }",
    },
    Feature {
        name: "strings",
        params: &[param("@A", 3, 40), param("@B", 1, 25)],
        template: "function f@K(i) { let s = ''; for (let j = 0; j < @A; j++) s += String.fromCharCode(97 + ((i + j * @B) % 26)); return s.slice(1, -1) + s.charCodeAt(i % s.length) + s.indexOf('c') + `${s}:${i}`.length + (s < 'm' ? 1 : 0) + s.toUpperCase().charCodeAt(0) + s.split('').reverse().join('').length; }",
    },
];
