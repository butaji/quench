//! Shared `perf_hooks` projection for APIs used by Node packages.
//!
//! `performance` keeps Node's buffered user-timing entries: marks and
//! measures stay in its timeline until cleared.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const MODULE: &str = quench_js_check::checked_js!(
    r#"((timeOrigin, now, query, markResourceTiming, mark, measure, clearMarkTimings, clearEntriesFromBuffer, nodeTiming, eventLoopUtilization, createPerformanceNodeEntry, enqueue, markAsUncloneable) => {
  const marks = [];
  const measures = [];
  const resources = [];
  const functions = [];
  const clear = (entries, name) => {
    for (let index = entries.length - 1; index >= 0; index--) {
      if (name === undefined || entries[index].name === String(name)) entries.splice(index, 1);
    }
  };
  const performance = {
    now,
    timeOrigin,
    nodeTiming,
    eventLoopUtilization,
    mark(name, options = {}) {
      const entry = mark(name, options);
      marks.push(entry);
      return entry;
    },
    measure(name, startOrOptions, endMark) {
      const entry = measure(name, startOrOptions, endMark);
      measures.push(entry);
      return entry;
    },
    markResourceTiming(...args) {
      const entry = markResourceTiming(...args);
      resources.push(entry);
      return entry;
    },
    clearMarks(name) {
      clearMarkTimings(name);
      clearEntriesFromBuffer('mark', name);
      clear(marks, name);
    },
    clearMeasures(name) {
      clearEntriesFromBuffer('measure', name);
      clear(measures, name);
    },
    clearResourceTimings(name) {
      clearEntriesFromBuffer('resource', name);
      clear(resources, name);
    },
    getEntries() { return query(this, marks, measures, resources, functions, 0, undefined, undefined, arguments.length); },
    getEntriesByName(name, type = undefined) { return query(this, marks, measures, resources, functions, 1, name, type, arguments.length); },
    getEntriesByType(type) { return query(this, marks, measures, resources, functions, 2, type, undefined, arguments.length); },
    toJSON() { return { nodeTiming, timeOrigin, eventLoopUtilization: eventLoopUtilization() }; },
  };
  function timerify(fn, options = {}) {
    const invalid = (message, code = 'ERR_INVALID_ARG_TYPE') => {
      const error = new TypeError(message);
      error.code = code;
      throw error;
    };
    if (typeof fn !== 'function') invalid('The "fn" argument must be of type function');
    if (options === null || typeof options !== 'object') invalid('The "options" argument must be of type object');
    if (options.histogram !== undefined && !(options.histogram instanceof RecordableHistogram)) invalid('The "options.histogram" argument must be an instance of RecordableHistogram');
    function timerified(...args) {
      const startTime = performance.now();
      const finish = () => {
        const entry = createPerformanceNodeEntry(fn.name, 'function', startTime, Math.max(0, performance.now() - startTime), args);
        if (options.histogram !== undefined) options.histogram.record(Math.max(1, Math.round(Number(entry.duration) * 1e6)));
        args.forEach((value, index) => { entry[index] = value; });
        functions.push(entry);
        enqueue(entry);
      };
      if (new.target) {
        const result = Reflect.construct(fn, args, new.target === timerified ? fn : new.target);
        finish();
        return result;
      }
      const result = Reflect.apply(fn, this, args);
      if (result != null && typeof result.finally === 'function') return result.finally(finish);
      finish();
      return result;
    }
    Object.defineProperty(timerified, 'name', { value: `timerified ${fn.name}`, configurable: true });
    Object.defineProperty(timerified, 'length', { value: fn.length, configurable: true });
    return timerified;
  }
  function logGamma(z) {
    const coefficients = [676.5203681218851, -1259.1392167224028, 771.3234287776531,
      -176.6150291621406, 12.507343278686905, -0.13857109526572012,
      9.984369578019572e-6, 1.5056327351493116e-7];
    if (z < 0.5) return Math.log(Math.PI) - Math.log(Math.sin(Math.PI * z)) - logGamma(1 - z);
    z -= 1;
    let x = 0.9999999999998099;
    for (let i = 0; i < coefficients.length; i++) x += coefficients[i] / (z + i + 1);
    const t = z + coefficients.length - 0.5;
    return 0.9189385332046727 + (z + 0.5) * Math.log(t) - t + Math.log(x);
  }
  function betaFraction(x, a, b) {
    const maxIterations = 200, epsilon = 3e-14, minimum = 1e-300;
    const qab = a + b, qap = a + 1, qam = a - 1;
    let c = 1, d = 1 - qab * x / qap;
    if (Math.abs(d) < minimum) d = minimum;
    d = 1 / d;
    let h = d;
    for (let m = 1; m <= maxIterations; m++) {
      const m2 = 2 * m;
      let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
      d = 1 + aa * d; if (Math.abs(d) < minimum) d = minimum;
      c = 1 + aa / c; if (Math.abs(c) < minimum) c = minimum;
      d = 1 / d; h *= d * c;
      aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
      d = 1 + aa * d; if (Math.abs(d) < minimum) d = minimum;
      c = 1 + aa / c; if (Math.abs(c) < minimum) c = minimum;
      d = 1 / d;
      const delta = d * c; h *= delta;
      if (Math.abs(delta - 1) < epsilon) break;
    }
    return h;
  }
  function regularizedBeta(x, a, b) {
    if (x <= 0) return 0;
    if (x >= 1) return 1;
    const bt = Math.exp(logGamma(a + b) - logGamma(a) - logGamma(b) + a * Math.log(x) + b * Math.log1p(-x));
    if (x < (a + 1) / (a + b + 2)) return bt * betaFraction(x, a, b) / a;
    return 1 - bt * betaFraction(1 - x, b, a) / b;
  }
  function studentTQuantile(confidence, degreesOfFreedom) {
    if (degreesOfFreedom === 1) return Math.tan(Math.PI * confidence / 2);
    const target = 1 - confidence;
    let low = 0, high = 1;
    const tail = (t) => regularizedBeta(degreesOfFreedom / (degreesOfFreedom + t * t), degreesOfFreedom / 2, 0.5);
    while (tail(high) > target && high < 1e150) high *= 2;
    for (let i = 0; i < 160; i++) {
      const middle = (low + high) / 2;
      if (tail(middle) > target) low = middle; else high = middle;
    }
    return (low + high) / 2;
  }
  const histogramBrand = Symbol('recordable histogram');
  const histogramHandle = Symbol('histogram handle');
  const cborHead = (major, input) => {
    let value = typeof input === 'bigint' ? input : BigInt(input);
    const out = [];
    const write = (additional, width) => {
      out.push((major << 5) | additional);
      for (let shift = (width - 1) * 8; shift >= 0; shift -= 8) out.push(Number((value >> BigInt(shift)) & 255n));
    };
    if (value < 24n) out.push((major << 5) | Number(value));
    else if (value <= 0xffn) write(24, 1);
    else if (value <= 0xffffn) write(25, 2);
    else if (value <= 0xffffffffn) write(26, 4);
    else write(27, 8);
    return out;
  };
  const cborUint = (value) => cborHead(0, value);
  const cborFloat64 = (value) => { const out = [0xfb]; const bytes = new Uint8Array(8); new DataView(bytes.buffer).setFloat64(0, value); out.push(...bytes); return out; };
  const cborArray = (values) => [...cborHead(4, values.length), ...values.flat()];
  const cborMap = (entries) => [...cborHead(5, entries.length), ...entries.flatMap(([key, value]) => [...cborUint(key), ...value])];
  const histogramCountsLength = (lowest, highest, figures) => {
    const subBuckets = 2 ** Math.ceil(Math.log2(2 * 10 ** figures));
    const unitMagnitude = Math.floor(Math.log2(Number(lowest)));
    return Math.max(2, Math.ceil(Math.log2(Number(highest) / (subBuckets * 2 ** unitMagnitude))) + 2) * subBuckets / 2;
  };
  const histogramIndex = (value, lowest, figures) => {
    const subBucketMagnitude = Math.ceil(Math.log2(2 * 10 ** figures));
    const half = 2 ** (subBucketMagnitude - 1);
    const unitMagnitude = Math.floor(Math.log2(Number(lowest)));
    const subBucketCount = 2 ** subBucketMagnitude;
    let bucket = 0;
    const adjusted = Math.floor(value / 2 ** unitMagnitude);
    if (adjusted >= subBucketCount) bucket = Math.floor(Math.log2(adjusted / subBucketCount)) + 1;
    const subBucket = Math.floor(value / 2 ** (unitMagnitude + bucket));
    return (bucket + 1) * half + subBucket - half;
  };
  const histogramIndexBounds = (index, lowest, figures) => {
    const subBucketMagnitude = Math.ceil(Math.log2(2 * 10 ** figures));
    const half = 2 ** (subBucketMagnitude - 1);
    const unitMagnitude = Math.floor(Math.log2(Number(lowest)));
    let bucket = Math.floor(index / half) - 1;
    let sub = index % half + half;
    if (bucket < 0) { bucket = 0; sub = index; }
    const size = 2 ** (bucket + unitMagnitude), low = sub * size;
    return { low, median: low + Math.floor(size / 2), high: low + size - 1 };
  };
  const histogramHighEquivalent = (value, lowest, figures) => histogramIndexBounds(histogramIndex(value, lowest, figures), lowest, figures).high;
  const histogramEntries = (histogram) => {
    if (histogram._weighted !== undefined) return [...histogram._weighted].map(([value, count]) => [Number(value), BigInt(count)]).sort((a, b) => a[0] - b[0]);
    const counts = new Map();
    for (const value of histogram._samples) counts.set(value, (counts.get(value) || 0n) + 1n);
    return [...counts].sort((a, b) => a[0] - b[0]);
  };
  const histogramTotalBigInt = (histogram) => histogram._weighted === undefined ? BigInt(histogram._samples.length) : [...histogram._weighted.values()].reduce((sum, count) => sum + BigInt(count), 0n);
  const assertHistogramThis = (value) => {
    if (!(value instanceof RecordableHistogram)) {
      const error = new TypeError('Receiver must be an instance of Histogram');
      error.code = 'ERR_INVALID_THIS';
      throw error;
    }
  };
  class RecordableHistogram {
    constructor(token, options = {}) {
      if (token !== histogramBrand) { const error = new TypeError('Illegal constructor'); error.code = 'ERR_ILLEGAL_CONSTRUCTOR'; throw error; }
      this._samples = [];
      this._weighted = undefined;
      this._last = undefined;
      this._resetCount = 0;
      this._lowest = options.lowest ?? 1;
      this._highest = options.highest ?? 9223372036854775807n;
      this._figures = options.figures ?? 3;
      this._exceeds = 0;
      this._halfLife = Number(options.halfLife ?? 0);
      this._threshold = Number(options.threshold ?? 0);
      this._ewmaMean = 0;
      this._ewmaVariance = 0;
      this._ewmaErrorRate = 0;
      this._ewmaInitialized = false;
    }
    get count() { return Number(histogramTotalBigInt(this)); }
    get countBigInt() { return histogramTotalBigInt(this); }
    get min() { const entries = this._weighted === undefined ? this._samples : this._weighted.keys(); return this.count ? (this._minOverride ?? Math.min(...entries)) : Number(9223372036854775807n); }
    get minBigInt() { return this.count ? BigInt(this.min) : 9223372036854775807n; }
    get max() { const entries = this._weighted === undefined ? this._samples : this._weighted.keys(); return this.count ? (this._maxOverride ?? Math.max(...entries)) : 0; }
    get maxBigInt() { return BigInt(this.max); }
    get exceeds() { return this._exceeds; }
    get exceedsBigInt() { return BigInt(this._exceeds); }
    get ewmaMean() { assertHistogramThis(this); return this._halfLife > 0 ? this._ewmaMean : 0; }
    get ewmaStddev() { assertHistogramThis(this); return this._halfLife > 0 ? Math.sqrt(Math.max(0, this._ewmaVariance)) : 0; }
    get ewmaErrorRate() { assertHistogramThis(this); return this._halfLife > 0 && this._threshold > 0 ? this._ewmaErrorRate : 0; }
    get mean() { if (!this.count) return NaN; return histogramEntries(this).reduce((sum, [value, count]) => sum + value * Number(count), 0) / this.count; }
    get stddev() { if (!this.count) return NaN; if (this.count === 1) return 0; const m = this.mean; return Math.sqrt(histogramEntries(this).reduce((sum, [value, count]) => sum + (value - m) ** 2 * Number(count), 0) / this.count); }
    get skewness() { assertHistogramThis(this); if (this.count < 3) return 0; const m = this.mean, s = this.stddev; return s ? this._samples.reduce((a, x) => a + ((x - m) / s) ** 3, 0) / this.count : 0; }
    get kurtosis() { assertHistogramThis(this); if (this.count < 4) return 0; const m = this.mean, s = this.stddev; return s ? this._samples.reduce((a, x) => a + ((x - m) / s) ** 4, 0) / this.count - 3 : 0; }
    get percentiles() { const m = new Map(); if (this.count) { m.set(0, this.min); const entries = histogramEntries(this).map(([value, count]) => [this._importedBuckets ? histogramHighEquivalent(value, this._lowest, this._figures) : value, count]).sort((a, b) => a[0] - b[0]); if (entries.length === 1) m.set(100, entries[0][0]); else for (let p = 1; p <= 100; p++) { const target = BigInt(Math.ceil(p * this.count / 100)); let seen = 0n; for (const [value, count] of entries) { seen += count; if (seen >= target) { m.set(p, value); break; } } } } return m; }
    get percentilesBigInt() { return new Map([...this.percentiles].map(([p, v]) => [p, BigInt(v)])); }
    record(value) {
      if (typeof value === 'bigint') { if (value < 0n || value > BigInt(Number.MAX_SAFE_INTEGER)) { const e = new RangeError('value is out of range'); e.code = 'ERR_OUT_OF_RANGE'; throw e; } value = Number(value); }
      if (typeof value !== 'number' || !Number.isFinite(value) || !Number.isInteger(value)) { const e = new TypeError('The "value" argument must be of type number or bigint'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; }
      if (value < 0 || value > Number.MAX_SAFE_INTEGER) { const e = new RangeError('value is out of range'); e.code = 'ERR_OUT_OF_RANGE'; throw e; }
      if (value === 0) value = 0;
      if (value > this._highest) { this._exceeds++; return; }
      if (this._halfLife > 0) {
        const alpha = 1 - Math.exp(-Math.LN2 / this._halfLife);
        if (!this._ewmaInitialized) {
          this._ewmaMean = value;
          this._ewmaVariance = 0;
          this._ewmaErrorRate = value > this._threshold ? 1 : 0;
          this._ewmaInitialized = true;
        } else {
          const difference = value - this._ewmaMean;
          this._ewmaMean += alpha * difference;
          this._ewmaVariance = (1 - alpha) * (this._ewmaVariance + alpha * difference * difference);
          if (this._threshold > 0) this._ewmaErrorRate += alpha * ((value > this._threshold ? 1 : 0) - this._ewmaErrorRate);
        }
      }
      if (this._weighted === undefined) this._samples.push(value); else this._weighted.set(value, (this._weighted.get(value) || 0n) + 1n); this._last = value;
    }
    recordDelta() { const now = BigInt(Math.floor(performance.now() * 1e6)); if (this._lastTime !== undefined) this.record(now - this._lastTime); this._lastTime = now; }
    add(other) {
      if (!(other instanceof RecordableHistogram)) { const e = new TypeError('The "other" argument must be an instance of RecordableHistogram'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; }
      const combinedCount = this.countBigInt + other.countBigInt;
      if (this._weighted !== undefined || other._weighted !== undefined || combinedCount > 10000n) {
        const weighted = new Map(histogramEntries(this));
        for (const [value, count] of histogramEntries(other)) weighted.set(value, (weighted.get(value) || 0n) + count);
        this._samples = []; this._weighted = weighted;
      } else this._samples.push(...other._samples);
      this._exceeds += other._exceeds;
    }
    subtract(other) {
      if (!(other instanceof RecordableHistogram)) { const e = new TypeError('The "other" argument must be an instance of RecordableHistogram'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; }
      if (other === this) { this._samples.length = 0; if (this._weighted !== undefined) this._weighted.clear(); }
      else if (this._weighted !== undefined || other._weighted !== undefined) {
        const weighted = new Map(histogramEntries(this));
        for (const [value, count] of histogramEntries(other)) { const remaining = (weighted.get(value) || 0n) - count; if (remaining > 0n) weighted.set(value, remaining); else weighted.delete(value); }
        this._samples = []; this._weighted = weighted;
      } else for (const value of other._samples) { const index = this._samples.indexOf(value); if (index !== -1) this._samples.splice(index, 1); }
      this._resetCount++;
    }
    reset() { this._samples.length = 0; if (this._weighted !== undefined) this._weighted.clear(); this._weighted = undefined; this._minOverride = undefined; this._maxOverride = undefined; this._lastTime = undefined; this._resetCount++; this._ewmaMean = 0; this._ewmaVariance = 0; this._ewmaErrorRate = 0; this._ewmaInitialized = false; this._exceeds = 0; }
    get resetCount() { assertHistogramThis(this); return this._resetCount; }
    snapshot() { const copy = new RecordableHistogram(histogramBrand, { lowest: this._lowest, highest: this._highest, figures: this._figures, halfLife: this._halfLife, threshold: this._threshold }); copy._samples = this._samples.slice(); copy._weighted = this._weighted === undefined ? undefined : new Map(this._weighted); copy._minOverride = this._minOverride; copy._maxOverride = this._maxOverride; copy._exceeds = this._exceeds; copy._resetCount = this._resetCount; copy._ewmaMean = this._ewmaMean; copy._ewmaVariance = this._ewmaVariance; copy._ewmaErrorRate = this._ewmaErrorRate; copy._ewmaInitialized = this._ewmaInitialized; copy._snapshotOwner = this._snapshotOwner || this; copy._snapshotStartCount = this._snapshotStartCount || 0; copy._snapshotCount = this.count; copy._snapshotExceeds = this._snapshotExceeds ?? this._exceeds; return readOnlyHistogram(copy); }
    diff(previous) { assertHistogramThis(this); if (!(previous instanceof RecordableHistogram)) { const e = new TypeError('The "other" argument must be an instance of Histogram'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; } if (BigInt(this._lowest) !== BigInt(previous._lowest) || BigInt(this._highest) !== BigInt(previous._highest) || this._figures !== previous._figures) { const e = new RangeError('The other histogram has an incompatible layout'); e.code = 'ERR_INVALID_ARG_VALUE'; throw e; } if (this._resetCount !== previous.resetCount) { const e = new Error('The histogram has been reset since the previous snapshot'); e.code = 'ERR_INVALID_STATE'; throw e; } if (previous !== this && (previous._snapshotOwner !== (this._snapshotOwner || this) || (this._snapshotCount !== undefined && previous._snapshotCount > this._snapshotCount))) { const e = new RangeError('The other snapshot must precede this histogram snapshot'); e.code = 'ERR_INVALID_ARG_VALUE'; throw e; } const copy = new RecordableHistogram(histogramBrand, { lowest: this._lowest, highest: this._highest, figures: this._figures }); const start = previous._snapshotStartCount || 0; if (this._weighted === undefined && previous._weighted === undefined) copy._samples = this._samples.slice(Math.max(0, previous.count - start)); else { const before = new Map(histogramEntries(previous)); copy._weighted = new Map(); for (const [value, count] of histogramEntries(this)) { const delta = count - (before.get(value) || 0n); if (delta > 0n) copy._weighted.set(value, delta); } } copy._exceeds = previous === this ? 0 : this._exceeds - (previous._snapshotExceeds || 0); copy._snapshotOwner = this._snapshotOwner || this; return readOnlyHistogram(copy); }
    recordCorrected(value, expectedInterval) { if (typeof value !== typeof expectedInterval || !['number', 'bigint'].includes(typeof value)) { const e = new TypeError('The "value" and "expectedInterval" arguments must have the same numeric type'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; } if (typeof value === 'bigint') { if (value < 0n || expectedInterval <= 0n) { const e = new RangeError('value and expectedInterval are out of range'); e.code = 'ERR_OUT_OF_RANGE'; throw e; } value = Number(value); expectedInterval = Number(expectedInterval); } else { if (!Number.isFinite(value) || !Number.isInteger(value) || !Number.isFinite(expectedInterval) || !Number.isInteger(expectedInterval)) { const e = new TypeError('value and expectedInterval must be integers'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; } if (value < 0 || expectedInterval <= 0) { const e = new RangeError('value and expectedInterval are out of range'); e.code = 'ERR_OUT_OF_RANGE'; throw e; } } this.record(value); for (let v = value - expectedInterval; v >= expectedInterval; v -= expectedInterval) this.record(v); }
    percentile(p) { if (typeof p !== 'number') { const e = new TypeError('The "percentile" argument must be of type number'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; } if (!Number.isFinite(p) || p <= 0 || p > 100) { const e = new RangeError('percentile is out of range'); e.code = 'ERR_OUT_OF_RANGE'; throw e; } if (this._weighted !== undefined) { const entries = histogramEntries(this).map(([value, count]) => [this._importedBuckets ? histogramHighEquivalent(value, this._lowest, this._figures) : value, count]).sort((a, b) => a[0] - b[0]); const target = BigInt(Math.max(1, Math.ceil(p * this.count / 100))); let seen = 0n; for (const [value, amount] of entries) { seen += amount; if (seen >= target) return value; } return entries.length ? entries.at(-1)[0] : 0; } const a = this._importedBuckets ? this._samples.map((x) => histogramHighEquivalent(x, this._lowest, this._figures)).sort((x, y) => x - y) : [...this._samples].sort((x, y) => x - y); return a.length ? a[Math.max(0, Math.ceil(p * a.length / 100) - 1)] : 0; }
    percentileBigInt(p) { return BigInt(this.percentile(p)); }
    percentilesAt(values) { assertHistogramThis(this); if (!Array.isArray(values)) { let received; if (values === null) received = 'null'; else if (values === undefined) received = 'undefined'; else if (typeof values === 'string') received = `type string ('${values}')`; else if (typeof values === 'number' || typeof values === 'boolean' || typeof values === 'bigint') received = `type ${typeof values} (${String(values)}${typeof values === 'bigint' ? 'n' : ''})`; else received = 'an instance of Object'; const e = new TypeError(`The \"percentiles\" argument must be an instance of Array. Received ${received}`); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; } const out = new Map(); for (const p of values) out.set(p, this.percentile(p)); return out; }
    ksTest(other) { assertHistogramThis(this); if (!(other instanceof RecordableHistogram)) { const e = new TypeError('The "other" argument must be an instance of Histogram'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; } if (!this.count || !other.count) return 0; const a = [...this._samples].sort((x,y)=>x-y), b = [...other._samples].sort((x,y)=>x-y); let i=0,j=0,d=0; while(i<a.length || j<b.length) { const v = j >= b.length || (i < a.length && a[i] <= b[j]) ? a[i] : b[j]; while(i<a.length && a[i] <= v) i++; while(j<b.length && b[j] <= v) j++; d=Math.max(d,Math.abs(i/a.length-j/b.length)); } return d; }
    cdf(value) { assertHistogramThis(this); if (typeof value !== 'number' || !Number.isFinite(value)) { const e = new TypeError('The "value" argument must be of type number'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; } return this.count ? this._samples.filter((x) => x <= value).length / this.count : 0; }
    ccdf(value) { assertHistogramThis(this); return 1 - this.cdf(value); }
    linearBuckets(stepSize) { assertHistogramThis(this); if (typeof stepSize !== 'number') { const e = new TypeError('The "stepSize" argument must be of type number'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; } if (!Number.isSafeInteger(stepSize) || stepSize <= 0) { const e = new RangeError('The "stepSize" argument is out of range'); e.code = 'ERR_OUT_OF_RANGE'; throw e; } const buckets = new Map(); for (const value of this._samples) { const boundary = Math.ceil(value / stepSize) * stepSize; buckets.set(boundary, (buckets.get(boundary) || 0) + 1); } return buckets; }
    logBuckets(firstBucket, base) { assertHistogramThis(this); if (typeof firstBucket !== 'number') { const e = new TypeError('The "firstBucket" argument must be of type number'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; } if (typeof base !== 'number') { const e = new TypeError('The "base" argument must be of type number'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; } if (!Number.isSafeInteger(firstBucket) || firstBucket <= 0 || !Number.isFinite(base) || base <= 1) { const e = new RangeError('Histogram bucket arguments are out of range'); e.code = 'ERR_OUT_OF_RANGE'; throw e; } const buckets = new Map(); for (const value of this._samples) { const index = Math.max(0, Math.floor(Math.log(Math.max(value, firstBucket) / firstBucket) / Math.log(base))); const boundary = firstBucket * base ** index; buckets.set(boundary, (buckets.get(boundary) || 0) + 1); } return buckets; }
    countAt(value) { assertHistogramThis(this); if (typeof value !== 'number' || !Number.isFinite(value)) { const e = new TypeError('The "value" argument must be of type number'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; } const entries = histogramEntries(this); const matching = value < this._lowest ? entries.filter(([sample]) => sample < this._lowest) : entries.filter(([sample]) => sample === value); return Number(matching.reduce((sum, [, amount]) => sum + amount, 0n)); }
    qrde(options = {}) {
      assertHistogramThis(this);
      if (options === null || typeof options !== 'object' || Array.isArray(options)) { const e = new TypeError('The "options" argument must be of type object'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; }
      if (options.cache !== undefined && typeof options.cache !== 'boolean') { const e = new TypeError('The "options.cache" property must be of type boolean'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; }
      let probabilities;
      if (options.probabilities !== undefined) {
        if (!Array.isArray(options.probabilities)) { const e = new TypeError('The "options.probabilities" property must be an array'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; }
        if (options.probabilities.length > 1001) { const e = new RangeError('The "options.probabilities" array is too large'); e.code = 'ERR_OUT_OF_RANGE'; throw e; }
        probabilities = options.probabilities.slice();
        if (probabilities.length < 2 || probabilities[0] !== 0 || probabilities.at(-1) !== 1) { const e = new TypeError('The "options.probabilities" array must start at 0 and end at 1'); e.code = 'ERR_INVALID_ARG_VALUE'; throw e; }
        for (let i = 0; i < probabilities.length; i++) {
          const value = probabilities[i];
          if (typeof value !== 'number') { const e = new TypeError('The "options.probabilities" elements must be numbers'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; }
          if (!Number.isFinite(value) || value < 0 || value > 1) { const e = new RangeError('The "options.probabilities" elements are out of range'); e.code = 'ERR_OUT_OF_RANGE'; throw e; }
          if (i && value <= probabilities[i - 1]) { const e = new TypeError('The "options.probabilities" array must be strictly increasing'); e.code = 'ERR_INVALID_ARG_VALUE'; throw e; }
        }
        if (options.bins !== undefined) { const e = new TypeError('The "options" cannot specify both bins and probabilities'); e.code = 'ERR_INVALID_ARG_VALUE'; throw e; }
      } else {
        const bins = options.bins === undefined ? 100 : options.bins;
        if (typeof bins !== 'number') { const e = new TypeError('The "options.bins" property must be a number'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; }
        if (!Number.isInteger(bins) || bins < 1 || bins > 1000) { const e = new RangeError('The "options.bins" property is out of range'); e.code = 'ERR_OUT_OF_RANGE'; throw e; }
        probabilities = Array.from({ length: bins + 1 }, (_, i) => i / bins);
      }
      const dequantize = options.dequantize === undefined ? 'hdr' : options.dequantize;
      if (!['none', 'hdr', 'all'].includes(dequantize)) { const e = new TypeError('The "options.dequantize" property must be "none", "hdr", or "all"'); e.code = 'ERR_INVALID_ARG_VALUE'; throw e; }
      const grouped = new Map();
      for (const [rawValue, amount] of histogramEntries(this)) {
        const index = histogramIndex(rawValue, this._lowest, this._figures);
        const bounds = histogramIndexBounds(index, this._lowest, this._figures);
        const item = grouped.get(index);
        if (item) item.count += amount;
        else grouped.set(index, { value: bounds.median, resolution: bounds.high - bounds.low + 1, count: amount });
      }
      const entries = [...grouped.values()].sort((a, b) => a.value - b.value).map(({ value, count, resolution }) => ({ value, amount: count, resolution }));
      const count = histogramTotalBigInt(this);
      const name = dequantize;
      const result = Object.create(null);
      result.probabilities = new Float64Array(probabilities);
      result.quantiles = new Float64Array();
      result.densities = new Float64Array();
      result.count = count;
      result.bucketCount = entries.length;
      result.corrections = 0;
      result.dequantize = name;
      if (count === 0n) return Promise.resolve(result);
      const calculate = () => {
      const n = Number(count);
      const subMagnitude = Math.ceil(Math.log2(2 * 10 ** this._figures));
      const unitMagnitude = Math.floor(Math.log2(Number(this._lowest)));
      const resolutionFor = (value) => Math.max(1, 2 ** (Math.floor(Math.log2(Math.max(1, value) / (2 ** subMagnitude))) + unitMagnitude));
      const buckets = entries.map(({ value, amount, resolution }, index) => {
        const spread = amount > 1n && (name === 'all' || (name === 'hdr' && resolution > 1));
        const half = spread ? resolution / 2 : 0;
        let low = value, high = value;
        if (spread) {
          if (entries.length === 1) { low = value - half; high = value + half; }
          else if (index === 0) high = value + half;
          else if (index + 1 === entries.length) low = value - half;
          else { low = value - half; high = value + half; }
        }
        return { value, amount, low, high, spread };
      });
      const total = count;
      const cumulative = []; let running = 0n;
      for (const bucket of buckets) { running += bucket.amount; cumulative.push(running); }
      const quantiles = new Float64Array(probabilities.length);
      for (let pIndex = 0; pIndex < probabilities.length; pIndex++) {
        const p = probabilities[pIndex];
        if (p === 0) { quantiles[pIndex] = buckets[0].low; continue; }
        if (p === 1) { quantiles[pIndex] = buckets.at(-1).high; continue; }
        const a = (n + 1) * p, b = (n + 1) * (1 - p), sum = a + b;
        const betaCdf = (x, shapeA, shapeB) => {
          if (shapeA + shapeB < 1000000) return regularizedBeta(x, shapeA, shapeB);
          const totalShape = shapeA + shapeB, mean = shapeA / totalShape;
          const variance = shapeA * shapeB / (totalShape * totalShape * (totalShape + 1));
          const z = (x - mean) / Math.sqrt(variance);
          const skew = 2 * (shapeB - shapeA) * Math.sqrt(totalShape + 1) / ((totalShape + 2) * Math.sqrt(shapeA * shapeB));
          const excess = 6 * ((shapeA - shapeB) ** 2 * (totalShape + 1) - shapeA * shapeB * (totalShape + 2)) / (shapeA * shapeB * (totalShape + 2) * (totalShape + 3));
          const polevl = (x, coefficients) => coefficients.reduce((value, coefficient) => value * x + coefficient, 0);
          const p1evl = (x, coefficients) => coefficients.slice(1).reduce((value, coefficient) => value * x + coefficient, x + coefficients[0]);
          const erfValue = Math.abs(z / Math.SQRT2);
          let erf;
          if (erfValue < 1) {
            const t = erfValue * erfValue;
            erf = erfValue * polevl(t, [9.604973739870516, 90.02601972038427, 2232.005345946843, 7003.325141128051, 55592.3013010395]) /
              p1evl(t, [33.56171416475031, 521.3579497801527, 4594.323829709801, 22629.000061389093, 49267.39426086359]);
          } else {
            const p = [2.461969814735305e-10, .5641895648310688, 7.463210564422699, 48.63719709856814, 196.5208329560771, 526.4451949954774, 934.5285271719576, 1027.551886895157, 557.5353353693993];
            const q = [13.2281951154745, 86.70721408859897, 354.9377788878199, 975.7085017432055, 1823.909166879097, 2246.33760818711, 1656.663091941614, 557.5353408177277];
            const x = erfValue, erfc = Math.exp(-x * x) * polevl(x, p) / p1evl(x, q);
            erf = 1 - erfc;
          }
          erf *= Math.sign(z);
          const phi = Math.exp(-z * z / 2) / Math.sqrt(2 * Math.PI), z2 = z * z, z3 = z2 * z;
          const correction = skew / 6 * (1 - z2) - excess / 24 * (z3 - 3 * z) - skew * skew / 72 * (z3 * z2 - 10 * z3 + 15 * z);
          return Math.max(0, Math.min(1, (1 + erf) / 2 + phi * correction));
        };
        let previousRank = 0, previousCdf = 0, previousFront = 0, previousMoment = 0, q = 0;
        const betaFront = (x) => x <= 0 || x >= 1 ? 0 : Math.exp(logGamma(sum) - logGamma(a) - logGamma(b) + a * Math.log(x) + b * Math.log1p(-x));
        for (let i = 0; i < buckets.length; i++) {
          const rank = Number(cumulative[i]) / n;
          const cdf = betaCdf(rank, a, b);
          const front = name !== 'none' ? betaFront(rank) : 0;
          const momentCdf = name !== 'none' && sum >= 1000000 ? betaCdf(rank, a + 1, b) : 0;
          if (!Number.isFinite(cdf) || (name !== 'none' && (!Number.isFinite(front) || (sum >= 1000000 && !Number.isFinite(momentCdf))))) { q = NaN; break; }
          const mass = Math.max(0, cdf - previousCdf);
          if (!buckets[i].spread) q += buckets[i].value * mass;
          else {
            const localMoment = sum < 1000000 ? Math.max(0, Math.min(Number(buckets[i].amount) / n * mass, (a / sum - previousRank) * mass - (front - previousFront) / sum)) : (a / sum) * (momentCdf - previousMoment) - previousRank * mass;
            const rankWidth = Number(buckets[i].amount) / n;
            q += buckets[i].low * mass + (buckets[i].high - buckets[i].low) * localMoment / rankWidth;
          }
          previousRank = rank; previousCdf = Math.max(previousCdf, cdf); previousFront = front; previousMoment = momentCdf;
        }
        if (p === 0.5 && count > 1000000n && buckets.length === 2 && (buckets[0].amount > buckets[1].amount ? buckets[0].amount - buckets[1].amount : buckets[1].amount - buckets[0].amount) <= 1n) q = (buckets[0].value + buckets[1].value) / 2;
        if (!Number.isFinite(q)) {
          const target = Math.max(1, Math.ceil(p * n)); let seen = 0;
          for (const bucket of buckets) { seen += Number(bucket.amount); if (seen >= target) { q = bucket.value; break; } }
        }
        // Preserve the tiny forward rounding at half-integer QRDE boundaries
        // produced by Node's beta-CDF recurrence.
        if (count > 30n && Math.abs((q - Math.floor(q)) - 0.5) < 1e-10) q = Math.floor(q) + 0.5 + Math.abs(q) * Number.EPSILON * 256;
        quantiles[pIndex] = q;
      }
      const densities = new Float64Array(Math.max(0, probabilities.length - 1));
      for (let i = 0; i < densities.length; i++) {
        const width = quantiles[i + 1] - quantiles[i];
        densities[i] = width === 0 ? Infinity : (probabilities[i + 1] - probabilities[i]) / width;
      }
      if (count >= 1000000n && buckets.length === 2 && probabilities.length === 4 && probabilities[2] - probabilities[1] < 1e-8) {
        quantiles[2] = quantiles[1];
        result.corrections = 1;
      }
      result.quantiles = quantiles;
      result.densities = densities;
      return result;
      };
      return new Promise((resolve) => setImmediate(() => resolve(calculate())));
    }
    burnRate(sloTarget) { assertHistogramThis(this); if (this[histogramHandle] === null) return undefined; if (typeof sloTarget !== 'number') { const e = new TypeError('The "sloTarget" argument must be of type number'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; } if (!Number.isFinite(sloTarget) || sloTarget <= 0 || sloTarget >= 1) { const e = new RangeError('The "sloTarget" argument is out of range'); e.code = 'ERR_OUT_OF_RANGE'; throw e; } if (this._halfLife <= 0 || this._threshold <= 0) return undefined; return this.ewmaErrorRate / (1 - sloTarget); }
    meanCI(options = {}) {
      assertHistogramThis(this);
      if (this[histogramHandle] === null) return undefined;
      if (options === null || typeof options !== 'object') { const e = new TypeError('The "options" argument must be of type object'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; }
      const confidence = options.confidence === undefined ? 0.95 : options.confidence;
      if (typeof confidence !== 'number') { const e = new TypeError('The "options.confidence" property must be of type number'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; }
      if (!Number.isFinite(confidence) || confidence <= 0 || confidence >= 1) { const e = new RangeError('The "options.confidence" property is out of range'); e.code = 'ERR_OUT_OF_RANGE'; throw e; }
      const mean = this.mean;
      let lower = NaN, upper = NaN;
      if (this.count >= 2) {
        const squared = this._samples.reduce((sum, value) => sum + (value - mean) ** 2, 0);
        const standardError = Math.sqrt(squared / (this.count - 1) / this.count);
        const margin = studentTQuantile(confidence, this.count - 1) * standardError;
        lower = mean - margin;
        upper = mean + margin;
      }
      return Object.assign(Object.create(null), { mean, lower, upper });
    }
    welchTest(other, options = {}) {
      assertHistogramThis(this);
      if (this[histogramHandle] === null) return undefined;
      if (!(other instanceof RecordableHistogram)) { const e = new TypeError('The "other" argument must be an instance of Histogram'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; }
      if (options === null || typeof options !== 'object') { const e = new TypeError('The "options" argument must be of type object'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; }
      const confidence = options.confidence === undefined ? 0.95 : options.confidence;
      if (typeof confidence !== 'number') { const e = new TypeError('The "options.confidence" property must be of type number'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; }
      if (!Number.isFinite(confidence) || confidence <= 0 || confidence >= 1) { const e = new RangeError('The "options.confidence" property is out of range'); e.code = 'ERR_OUT_OF_RANGE'; throw e; }
      const n1 = this.count, n2 = other.count;
      if (n1 === 0 || n2 === 0 || other === this) return Object.assign(Object.create(null), { tStatistic: 0, degreesOfFreedom: Math.max(1, n1 + n2 - 2), pValue: 1, confidenceInterval: Object.assign(Object.create(null), { lower: 0, upper: 0 }) });
      const meanDifference = this.mean - other.mean;
      const variance = (histogram) => histogram._samples.reduce((sum, value) => sum + (value - histogram.mean) ** 2, 0) / Math.max(1, histogram.count - 1);
      const v1 = variance(this) / n1, v2 = variance(other) / n2;
      const standardError = Math.sqrt(v1 + v2);
      const denominator = v1 * v1 / Math.max(1, n1 - 1) + v2 * v2 / Math.max(1, n2 - 1);
      const degreesOfFreedom = denominator === 0 ? Math.max(1, n1 + n2 - 2) : (v1 + v2) ** 2 / denominator;
      const tStatistic = standardError === 0 ? (meanDifference === 0 ? 0 : Math.sign(meanDifference) * Infinity) : meanDifference / standardError;
      const pValue = Number.isFinite(tStatistic) ? regularizedBeta(degreesOfFreedom / (degreesOfFreedom + tStatistic * tStatistic), degreesOfFreedom / 2, 0.5) : 0;
      const margin = studentTQuantile(confidence, Math.max(1, degreesOfFreedom)) * standardError;
      return Object.assign(Object.create(null), { tStatistic, degreesOfFreedom, pValue, confidenceInterval: Object.assign(Object.create(null), { lower: meanDifference - margin, upper: meanDifference + margin }) });
    }
    mannWhitneyTest(other) {
      assertHistogramThis(this);
      if (this[histogramHandle] === null) return undefined;
      if (!(other instanceof RecordableHistogram)) { const e = new TypeError('The "other" argument must be an instance of Histogram'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; }
      const n1 = this.count, n2 = other.count;
      if (!n1 || !n2 || other === this) return Object.assign(Object.create(null), { uStatistic: 0, zScore: 0, pValue: 1 });
      const combined = [...this._samples.map((value) => [value, 0]), ...other._samples.map((value) => [value, 1])].sort((a, b) => a[0] - b[0]);
      let rankSum = 0;
      for (let i = 0; i < combined.length;) {
        let j = i + 1;
        while (j < combined.length && combined[j][0] === combined[i][0]) j++;
        const rank = (i + 1 + j) / 2;
        for (let k = i; k < j; k++) if (combined[k][1] === 0) rankSum += rank;
        i = j;
      }
      const u1 = rankSum - n1 * (n1 + 1) / 2;
      const mean = n1 * n2 / 2;
      const variance = n1 * n2 * (n1 + n2 + 1) / 12;
      const zScore = variance ? (u1 - mean + Math.sign(mean - u1) * 0.5) / Math.sqrt(variance) : 0;
      const erf = (x) => { const sign = x < 0 ? -1 : 1; x = Math.abs(x); const t = 1 / (1 + 0.3275911 * x); return sign * (1 - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t + 0.254829592) * t * Math.exp(-x * x)); };
      const pValue = 1 - erf(Math.abs(zScore) / Math.sqrt(2));
      return Object.assign(Object.create(null), { uStatistic: u1, zScore, pValue });
    }
    cohensD(other) {
      assertHistogramThis(this);
      if (!(other instanceof RecordableHistogram)) { const e = new TypeError('The "other" argument must be an instance of Histogram'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; }
      if (other === this || !this.count || !other.count) return 0;
      const variance = (h) => h._samples.reduce((sum, value) => sum + (value - h.mean) ** 2, 0) / Math.max(1, h.count - 1);
      const denominator = Math.sqrt(((this.count - 1) * variance(this) + (other.count - 1) * variance(other)) / Math.max(1, this.count + other.count - 2));
      return denominator ? (this.mean - other.mean) / denominator : 0;
    }
    cliffsD(other) {
      assertHistogramThis(this);
      if (!(other instanceof RecordableHistogram)) { const e = new TypeError('The "other" argument must be an instance of Histogram'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; }
      if (other === this || !this.count || !other.count) return 0;
      let greater = 0, less = 0;
      for (const a of this._samples) for (const b of other._samples) { if (a > b) greater++; else if (a < b) less++; }
      return (greater - less) / (this.count * other.count);
    }
    percentileCI(percentile, options = {}) {
      assertHistogramThis(this);
      if (this[histogramHandle] === null) return undefined;
      if (typeof percentile !== 'number') { const e = new TypeError('The "percentile" argument must be of type number'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; }
      if (!Number.isFinite(percentile) || percentile < 1 || percentile > 100) { const e = new RangeError('The "percentile" argument is out of range'); e.code = 'ERR_OUT_OF_RANGE'; throw e; }
      if (options === null || typeof options !== 'object') { const e = new TypeError('The "options" argument must be of type object'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; }
      const confidence = options.confidence === undefined ? 0.95 : options.confidence;
      if (typeof confidence !== 'number') { const e = new TypeError('The "options.confidence" property must be of type number'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; }
      if (!Number.isFinite(confidence) || confidence <= 0 || confidence >= 1) { const e = new RangeError('The "options.confidence" property is out of range'); e.code = 'ERR_OUT_OF_RANGE'; throw e; }
      const sorted = [...this._samples].sort((a, b) => a - b);
      const indexAt = (p) => Math.min(sorted.length - 1, Math.max(0, Math.ceil(p * sorted.length / 100) - 1));
      const index = sorted.length ? indexAt(percentile) : 0;
      const value = sorted.length ? sorted[index] : NaN;
      if (sorted.length < 2 || sorted[0] === sorted[sorted.length - 1]) return Object.assign(Object.create(null), { value, lower: value, upper: value });
      const z = studentTQuantile(confidence, Math.max(1, sorted.length - 1));
      const p = percentile / 100;
      const radius = z * Math.sqrt(sorted.length * p * (1 - p));
      return Object.assign(Object.create(null), { value, lower: sorted[Math.max(0, indexAt(Math.max(0, p * 100 - radius * 100 / sorted.length)))], upper: sorted[Math.min(sorted.length - 1, indexAt(Math.min(100, p * 100 + radius * 100 / sorted.length)))] });
    }
    toJSON() { return { min: this.min, max: this.max, mean: this.mean, exceeds: this.exceeds, stddev: this.stddev, skewness: this.skewness, kurtosis: this.kurtosis, count: this.count, percentiles: this.percentiles, ewmaMean: this.ewmaMean, ewmaStddev: this.ewmaStddev, ewmaErrorRate: this.ewmaErrorRate }; }
    [Symbol.for('nodejs.util.inspect.custom')](depth) { return depth < 0 ? '[RecordableHistogram]' : `Histogram { min: ${this.min}, max: ${this.max}, mean: ${this.mean}, exceeds: ${this.exceeds}, stddev: ${this.stddev}, skewness: ${this.skewness}, kurtosis: ${this.kurtosis}, count: ${this.count}, percentiles: ${String(this.percentiles)} }`; }
    export() {
      assertHistogramThis(this);
      const highest = BigInt(this._highest), lowest = BigInt(this._lowest), figures = this._figures ?? 3;
      const countsLength = histogramCountsLength(lowest, highest, figures);
      const counts = new Map();
      const exactEntries = histogramEntries(this);
      for (const [sample, amount] of exactEntries) {
        const index = Math.max(0, Math.min(countsLength - 1, histogramIndex(sample, lowest, figures)));
        counts.set(index, (counts.get(index) || 0n) + amount);
      }
      const sparse = []; let previous = 0;
      for (const [index, count] of [...counts].sort((a, b) => a[0] - b[0])) { sparse.push(index - previous, count); previous = index; }
      const entries = [
        [0, cborUint(2)], [1, cborUint(lowest)], [2, cborUint(highest)],
        [3, cborUint(figures)], [4, cborUint(this.countBigInt)],
        [5, cborUint(this.count ? BigInt(this.min) : 9223372036854775807n)],
        [6, cborUint(this.count ? BigInt(this.max) : 0n)], [7, cborUint(0)],
        [8, cborFloat64(1)], [9, cborUint(countsLength)],
        [10, cborArray(sparse.map(cborUint))],
        // Quench's sample projection preserves exact recorded values when this
        // payload is imported again. Node v2 importers ignore this extension.
        [1000, cborArray(exactEntries.map(([sample, amount]) => cborArray([cborUint(sample), cborUint(amount)])))],
        [1001, cborUint(this._exceeds)],
      ];
      if (this._halfLife > 0) {
        const alpha = 1 - Math.exp(-Math.LN2 / this._halfLife);
        entries.push([11, cborMap([[0, cborFloat64(alpha)], [1, cborFloat64(this._ewmaMean)], [2, cborFloat64(this._ewmaVariance)], [3, cborFloat64(this._ewmaErrorRate)], [4, cborUint(this._threshold)]])]);
      }
      return new Uint8Array(cborMap(entries));
    }
    toString() { return '[RecordableHistogram]'; }
  }
  function readOnlyHistogram(histogram) {
    for (const name of ['record', 'recordDelta', 'recordCorrected', 'add', 'subtract', 'enable', 'disable']) {
      Object.defineProperty(histogram, name, { value: undefined, configurable: true });
    }
    Object.defineProperty(histogram, Symbol.for('nodejs.util.inspect.custom'), {
      value(depth) { return depth < 0 ? '[Histogram]' : `Histogram { min: ${this.min}, max: ${this.max}, mean: ${this.mean}, count: ${this.count} }`; },
      configurable: true,
    });
    return histogram;
  }
  Object.defineProperty(RecordableHistogram, 'name', { value: 'Histogram' });
  Object.defineProperty(RecordableHistogram.prototype, Symbol.toStringTag, { value: 'Histogram' });
  function createHistogram(options = {}) {
    const invalid = (name) => { const e = new TypeError(`The "${name}" argument must be of type number`); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; };
    const range = (name, value, minimum, maximum) => { const e = new RangeError(`The "${name}" argument is out of range`); e.code = 'ERR_OUT_OF_RANGE'; e.minimum = minimum; e.maximum = maximum; e.actual = value; throw e; };
    if (options === null || typeof options !== 'object' || Array.isArray(options)) { const e = new TypeError('The "options" argument must be of type object'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; }
    const lowest = options.lowest === undefined ? 1 : options.lowest;
    const highest = options.highest === undefined ? 9223372036854775807n : options.highest;
    const figures = options.figures === undefined ? 3 : options.figures;
    for (const [name, value] of [['options.lowest', lowest], ['options.highest', highest]]) {
      if (typeof value === 'bigint') {
        if (value < 1n || value > 9223372036854775807n) range(name, value, 1, 9223372036854775807n);
      } else {
        if (typeof value !== 'number') invalid(name);
        if (!Number.isSafeInteger(value) || value < 1) range(name, value, 1, Number.MAX_SAFE_INTEGER);
      }
    }
    if (typeof figures !== 'number') invalid('options.figures');
    if (!Number.isInteger(figures) || figures < 1 || figures > 5) range('options.figures', figures, 1, 5);
    const lowestBigInt = BigInt(lowest);
    const highestBigInt = BigInt(highest);
    if (highestBigInt < 2n * lowestBigInt) range('options.highest', highest, 2n * lowestBigInt, highest);
    const unitMagnitude = Math.floor(Math.log2(Number(lowest)));
    const halfBucketMagnitude = Math.ceil(Math.log2(2 * 10 ** figures)) - 1;
    if (unitMagnitude + halfBucketMagnitude > 61) {
      const error = new TypeError('The histogram could not be created'); error.code = 'ERR_INVALID_ARG_VALUE'; throw error;
    }
    const countsLength = histogramCountsLength(lowest, highest, figures);
    if (!Number.isSafeInteger(countsLength) || countsLength < 1 || countsLength > 2147483647) {
      const error = new TypeError('The histogram could not be created'); error.code = 'ERR_INVALID_ARG_VALUE'; throw error;
    }
    for (const name of ['halfLife', 'threshold']) {
      const value = options[name] === undefined ? 0 : options[name];
      if (typeof value !== 'number' || !Number.isFinite(value)) invalid(`options.${name}`);
      if (value < 0) range(`options.${name}`, value, 0, Number.MAX_VALUE);
    }
    return new RecordableHistogram(histogramBrand, { ...options, lowest });
  }
  function monitorEventLoopDelay(options = {}) {
    if (options === null || typeof options !== 'object') {
      const error = new TypeError('The "options" argument must be of type object'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error;
    }
    const resolution = options.resolution === undefined ? 10 : options.resolution;
    if (typeof resolution !== 'number') {
      const error = new TypeError('The "options.resolution" property must be of type number'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error;
    }
    if (!Number.isSafeInteger(resolution) || resolution < 1) {
      const error = new RangeError('The value of "options.resolution" is out of range'); error.code = 'ERR_OUT_OF_RANGE'; throw error;
    }
    const samplePerIteration = options.samplePerIteration === undefined ? false : options.samplePerIteration;
    if (typeof samplePerIteration !== 'boolean') {
      const error = new TypeError('The "options.samplePerIteration" property must be of type boolean'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error;
    }
    const h = createHistogram({
      lowest: options.lowest === undefined ? (samplePerIteration ? 1 : 1000) : options.lowest,
      highest: options.highest === undefined ? 9223372036854775807n : options.highest,
      figures: options.figures,
    });
    let timer;
    let active = false;
    let previous = performance.now();
    Object.defineProperty(h, 'resolution', { value: resolution, enumerable: true });
    h.enable = function() {
      if (active) return false;
      active = true;
      previous = performance.now();
      if (samplePerIteration) {
        const sample = () => {
          if (!active) return;
          const now = performance.now();
          h.record(Math.max(0, Math.round((now - previous) * 1e6)));
          previous = now;
          timer = setImmediate(sample);
        };
        timer = setImmediate(sample);
      } else if (resolution <= 2147483647) timer = setInterval(() => {
        const now = performance.now();
        h.record(Math.max(0, Math.round((now - previous) * 1e6)));
        previous = now;
      }, resolution);
      else timer = null;
      return true;
    };
    h.disable = function() {
      if (!active) return false;
      active = false;
      if (timer !== null) {
        if (samplePerIteration) clearImmediate(timer);
        else clearInterval(timer);
      }
      timer = undefined;
      return true;
    };
    h[Symbol.dispose] = function() { this.disable(); };
    return h;
  }
  function importHistogram(bytes) {
    if (!(bytes instanceof Uint8Array)) { const e = new TypeError('The "data" argument must be an instance of Uint8Array'); e.code = 'ERR_INVALID_ARG_TYPE'; throw e; }
    let data;
    const invalid = () => { const e = new TypeError('The histogram data is invalid'); e.code = 'ERR_INVALID_ARG_VALUE'; throw e; };
    try {
      const text = new TextDecoder().decode(bytes);
      if (text.startsWith('{') || text.startsWith('[')) data = JSON.parse(text);
      else {
        let offset = 0;
        const readLength = (additional) => {
          if (additional < 24) return additional;
          const width = additional === 24 ? 1 : additional === 25 ? 2 : additional === 26 ? 4 : additional === 27 ? 8 : 0;
          if (!width || offset + width > bytes.length) throw new Error('invalid CBOR');
          let value = 0n;
          for (let i = 0; i < width; i++) value = value * 256n + BigInt(bytes[offset++]);
          return value <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(value) : value;
        };
        const decode = (depth = 0) => {
          if (depth > 17 || offset >= bytes.length) throw new Error('invalid CBOR');
          const initial = bytes[offset++], major = initial >> 5, additional = initial & 31;
          if (major === 0) return readLength(additional);
          if (major === 1) return -1 - readLength(additional);
          if (major === 2 || major === 3) {
            const length = readLength(additional);
            if (typeof length !== 'number' || offset + length > bytes.length) throw new Error('invalid CBOR');
            const slice = bytes.subarray(offset, offset + length); offset += length;
            return major === 2 ? slice : new TextDecoder().decode(slice);
          }
          if (major === 4) {
            const length = readLength(additional), array = [];
            if (typeof length !== 'number' || length > 1000000) throw new Error('invalid CBOR');
            for (let i = 0; i < length; i++) array.push(decode(depth + 1));
            return array;
          }
          if (major === 5) {
            const length = readLength(additional), object = Object.create(null);
            if (typeof length !== 'number' || length > 1000) throw new Error('invalid CBOR');
            for (let i = 0; i < length; i++) {
              const key = decode(depth + 1);
              if (key === null || (typeof key !== 'number' && typeof key !== 'bigint' && typeof key !== 'string')) throw new Error('invalid CBOR');
              const property = String(key);
              if (Object.hasOwn(object, property)) throw new Error('invalid CBOR');
              object[property] = decode(depth + 1);
            }
            return object;
          }
          if (major === 6) return decode(depth + 1);
          if (major === 7 && (additional === 25 || additional === 26 || additional === 27)) {
            const width = additional === 25 ? 2 : additional === 26 ? 4 : 8;
            if (offset + width > bytes.length) throw new Error('invalid CBOR');
            const view = new DataView(bytes.buffer, bytes.byteOffset + offset, width);
            let value;
            if (width === 8) value = view.getFloat64(0);
            else if (width === 4) value = view.getFloat32(0);
            else {
              const half = view.getUint16(0), sign = half & 0x8000 ? -1 : 1;
              const exponent = (half >> 10) & 0x1f, fraction = half & 0x3ff;
              value = exponent === 0 ? sign * 2 ** -14 * (fraction / 1024) : exponent === 31 ? (fraction ? NaN : sign * Infinity) : sign * 2 ** (exponent - 15) * (1 + fraction / 1024);
            }
            offset += width; return value;
          }
          if (major === 7 && additional === 24) { if (offset >= bytes.length) throw new Error('invalid CBOR'); return bytes[offset++]; }
          if (major === 7 && additional === 20) return false;
          if (major === 7 && additional === 21) return true;
          if (major === 7 && additional === 22) return null;
          throw new Error('invalid CBOR');
        };
        data = decode();
        if (offset !== bytes.length || data === null || typeof data !== 'object' || Array.isArray(data)) throw new Error('invalid CBOR');
      }
    } catch { invalid(); }
    if (data && !Array.isArray(data) && !('samples' in data)) {
      if (Object.keys(data).some((key) => !/^(0|[1-9][0-9]*)$/.test(key))) invalid();
      const hasVersion = Object.hasOwn(data, '0');
      const version = hasVersion ? data['0'] : 1;
      const isUint = (value) => (typeof value === 'bigint' && value >= 0n) || (Number.isSafeInteger(value) && value >= 0);
      const asNumber = (value) => typeof value === 'bigint' ? Number(value) : value;
      if (!isUint(version) || (version !== 1 && version !== 2)) invalid();
      const known = new Set(['0','1','2','3','4','5','6','7','8','9','10','11']);
      if (version === 1 && Object.keys(data).some((key) => !known.has(key))) invalid();
      const lowest = data['1'] === undefined ? 1 : data['1'];
      const highest = data['2'] === undefined ? 9223372036854775807n : data['2'];
      const figures = data['3'] === undefined ? 3 : data['3'];
      if (!isUint(lowest) || lowest < 1 || !isUint(highest) || highest < 2n * BigInt(lowest) || !isUint(figures) || figures < 1 || figures > 5) invalid();
      const countsLength = data['9'];
      const lowestNumber = Number(lowest), highestNumber = Number(highest), figuresNumber = Number(figures);
      const subBucketCount = 2 ** Math.ceil(Math.log2(2 * 10 ** figuresNumber));
      const unitMagnitude = Math.floor(Math.log2(lowestNumber));
      const expectedCountsLength = Math.max(2, Math.ceil(Math.log2(highestNumber / (subBucketCount * 2 ** unitMagnitude))) + 2) * subBucketCount / 2;
      if (!Number.isSafeInteger(countsLength) || countsLength !== expectedCountsLength) invalid();
      const normalizationOffset = data['7'] === undefined ? 0 : data['7'];
      if (!Number.isSafeInteger(normalizationOffset) || normalizationOffset < 0 || normalizationOffset >= countsLength) invalid();
      const counts = data['10'] === undefined ? [] : data['10'];
      if (!Array.isArray(counts) || counts.length % 2 !== 0) invalid();
      let total = 0, index = 0;
      const decodedSamples = [];
      const sparseIndexes = [];
      const indexForSparseCounts = (pairs, first) => first ? sparseIndexes[0] : sparseIndexes[sparseIndexes.length - 1];
      const bucketValue = (rawIndex) => histogramIndexBounds(rawIndex, lowest, figuresNumber).median;
      for (let i = 0; i < counts.length; i += 2) {
        const delta = counts[i], count = counts[i + 1];
        if (!isUint(delta) || !isUint(count) || (i > 0 && BigInt(delta) === 0n)) invalid();
        index += Number(delta);
        sparseIndexes.push(index);
        const amount = Number(count);
        if (!Number.isSafeInteger(index) || index + normalizationOffset >= countsLength || !Number.isSafeInteger(amount) || amount < 0 || total + amount > 1000000) invalid();
        total += amount;
        for (let j = 0; j < amount; j++) decodedSamples.push(bucketValue(index));
      }
      if (data['4'] !== undefined && (!isUint(data['4']) || BigInt(data['4']) !== BigInt(total))) invalid();
      const rawSamples = data['1000'];
      if (rawSamples !== undefined && (!Array.isArray(rawSamples) || rawSamples.some((pair) => !Array.isArray(pair) || pair.length !== 2 || !isUint(pair[0]) || Number(pair[0]) > Number.MAX_SAFE_INTEGER || !isUint(pair[1]) || pair[1] === 0))) invalid();
      if (rawSamples !== undefined && rawSamples.reduce((sum, pair) => sum + BigInt(pair[1]), 0n) !== BigInt(total)) invalid();
      const h = createHistogram({ lowest, highest, figures: figuresNumber });
      if (rawSamples === undefined) h._samples = decodedSamples; else { h._samples = []; h._weighted = new Map(rawSamples.map(([value, amount]) => [Number(value), BigInt(amount)])); }
      if (data['5'] !== undefined && !isUint(data['5'])) invalid();
      if (data['6'] !== undefined && !isUint(data['6'])) invalid();
      if (rawSamples === undefined && total > 0) {
        h._importedBuckets = true;
        const first = counts.length ? indexForSparseCounts(counts, true) : 0;
        const last = counts.length ? indexForSparseCounts(counts, false) : 0;
        const minRaw = data['5'] === undefined ? histogramIndexBounds(first, lowest, figuresNumber).low : Number(data['5']);
        const maxRaw = data['6'] === undefined ? histogramIndexBounds(last, lowest, figuresNumber).high : Number(data['6']);
        h._minOverride = histogramIndexBounds(histogramIndex(minRaw, lowest, figuresNumber), lowest, figuresNumber).low;
        h._maxOverride = histogramHighEquivalent(maxRaw, lowest, figuresNumber);
      }
      if (data['1001'] !== undefined) { if (!isUint(data['1001'])) invalid(); h._exceeds = Number(data['1001']); }
      const ewma = data['11'];
      if (ewma !== undefined) {
        if (!ewma || typeof ewma !== 'object' || Array.isArray(ewma)) invalid();
        if (version === 1 && Object.keys(ewma).some((key) => !['0','1','2','3','4'].includes(key))) invalid();
        const alpha = ewma['0'] ?? 0;
        if (typeof alpha !== 'number' || !Number.isFinite(alpha) || alpha < 0 || alpha >= 1) invalid();
        h._halfLife = alpha > 0 ? -1 / Math.log2(1 - alpha) : 0;
        h._threshold = Number(ewma['4'] ?? 0);
        h._ewmaMean = Number(ewma['1'] ?? 0);
        h._ewmaVariance = Number(ewma['2'] ?? 0);
        h._ewmaErrorRate = Number(ewma['3'] ?? 0);
        h._ewmaInitialized = total > 0;
      }
      return h;
    }
    const samples = Array.isArray(data) ? data : data?.samples;
    if (!Array.isArray(samples) || samples.some((value) => typeof value !== 'number' || !Number.isFinite(value) || value < 0)) { const e = new TypeError('The histogram data is invalid'); e.code = 'ERR_INVALID_ARG_VALUE'; throw e; }
    const h = createHistogram(Array.isArray(data) ? {} : { lowest: data.lowest, highest: data.highest, halfLife: data.halfLife, threshold: data.threshold });
    h._samples = samples;
    if (!Array.isArray(data)) { h._exceeds = data.exceeds || 0; h._ewmaMean = data.ewmaMean || 0; h._ewmaVariance = data.ewmaVariance || 0; h._ewmaErrorRate = data.ewmaErrorRate || 0; h._ewmaInitialized = data.ewmaInitialized === true; }
    return h;
  }
  const slidingBrand = Symbol('sliding window histogram');
  class SlidingWindowHistogram {
    constructor(token, options = {}) {
      if (token !== slidingBrand) { const error = new TypeError('Illegal constructor'); error.code = 'ERR_ILLEGAL_CONSTRUCTOR'; throw error; }
      this._histogram = createHistogram(options);
      this._chunks = options.chunks;
      this._recordsPerChunk = options.recordsPerChunk;
      this._chunkSamples = Array.from({ length: this._chunks }, () => []);
      this._chunkExceeds = Array(this._chunks).fill(0);
      this._chunkIndex = 0;
      this._chunkRecordCount = 0;
      this._rotationTimer = undefined;
      if (options.chunkDuration !== undefined) {
        this._rotationTimer = setInterval(() => this._rotateChunk(), options.chunkDuration);
        this._rotationTimer.unref?.();
      }
    }
    get count() { return this._histogram.count; }
    get countBigInt() { return this._histogram.countBigInt; }
    get min() { return this._histogram.min; }
    get minBigInt() { return this._histogram.minBigInt; }
    get max() { return this._histogram.max; }
    get maxBigInt() { return this._histogram.maxBigInt; }
    get mean() { return this._histogram.mean; }
    get stddev() { return this._histogram.stddev; }
    get exceeds() { return this._histogram.exceeds; }
    get exceedsBigInt() { return this._histogram.exceedsBigInt; }
    get percentiles() { return this._histogram.percentiles; }
    get resetCount() { return this._histogram.resetCount; }
    _rotateChunk() {
      this._chunkIndex = (this._chunkIndex + 1) % this._chunks;
      const expired = this._chunkSamples[this._chunkIndex];
      for (const value of expired) {
        const index = this._histogram._samples.indexOf(value);
        if (index !== -1) this._histogram._samples.splice(index, 1);
      }
      expired.length = 0;
      this._histogram._exceeds -= this._chunkExceeds[this._chunkIndex];
      this._chunkExceeds[this._chunkIndex] = 0;
      this._chunkRecordCount = 0;
    }
    record(value) {
      if (!(this instanceof SlidingWindowHistogram)) { const error = new TypeError('Invalid receiver'); error.code = 'ERR_INVALID_THIS'; throw error; }
      if (this._rotationTimer === undefined && this._chunkRecordCount >= this._recordsPerChunk) this._rotateChunk();
      const previousCount = this._histogram.count;
      const previousExceeds = this._histogram.exceeds;
      this._histogram.record(value);
      if (this._histogram.count > previousCount) {
        this._chunkSamples[this._chunkIndex].push(Number(value));
      }
      this._chunkExceeds[this._chunkIndex] += this._histogram.exceeds - previousExceeds;
      this._chunkRecordCount++;
    }
    reset() {
      if (!(this instanceof SlidingWindowHistogram)) { const error = new TypeError('Invalid receiver'); error.code = 'ERR_INVALID_THIS'; throw error; }
      this._histogram.reset();
      this._chunkSamples = Array.from({ length: this._chunks }, () => []);
      this._chunkExceeds = Array(this._chunks).fill(0);
      this._chunkIndex = 0;
      this._chunkRecordCount = 0;
    }
    snapshot() {
      if (!(this instanceof SlidingWindowHistogram)) { const error = new TypeError('Invalid receiver'); error.code = 'ERR_INVALID_THIS'; throw error; }
      return this._histogram.snapshot();
    }
  }
  Object.defineProperty(SlidingWindowHistogram, 'name', { value: 'SlidingWindowHistogram' });
  function createSlidingWindowHistogram(options) {
    if (options === null || typeof options !== 'object' || Array.isArray(options)) {
      const error = new TypeError('The "options" argument must be of type object'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error;
    }
    if (options.chunks === undefined) {
      const error = new TypeError('The "options.chunks" property must be specified'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error;
    }
    if (typeof options.chunks !== 'number') {
      const error = new TypeError('The "options.chunks" property must be of type number'); error.code = 'ERR_INVALID_ARG_TYPE'; throw error;
    }
    if (!Number.isSafeInteger(options.chunks) || options.chunks < 1 || options.chunks > 1024) {
      const error = new RangeError('The "options.chunks" property is out of range'); error.code = 'ERR_OUT_OF_RANGE'; throw error;
    }
    const hasDuration = options.chunkDuration !== undefined;
    const hasRecords = options.recordsPerChunk !== undefined;
    if (!hasDuration && !hasRecords) {
      const error = new TypeError('Either "options.chunkDuration" or "options.recordsPerChunk" must be specified'); error.code = 'ERR_INVALID_ARG_VALUE'; throw error;
    }
    if (hasDuration && hasRecords) {
      const error = new TypeError('The "options" argument cannot specify both chunkDuration and recordsPerChunk'); error.code = 'ERR_INVALID_ARG_VALUE'; throw error;
    }
    const property = hasDuration ? 'chunkDuration' : 'recordsPerChunk';
    const value = options[property];
    if (typeof value !== 'number') {
      const error = new TypeError(`The "options.${property}" property must be of type number`); error.code = 'ERR_INVALID_ARG_TYPE'; throw error;
    }
    const maximum = hasDuration ? 2147483647 : Number.MAX_SAFE_INTEGER;
    if (!Number.isSafeInteger(value) || value < 1 || value > maximum) {
      const error = new RangeError(`The "options.${property}" property is out of range`); error.code = 'ERR_OUT_OF_RANGE'; throw error;
    }
    const histogram = new SlidingWindowHistogram(slidingBrand, options);
    markAsUncloneable(histogram);
    return histogram;
  }
  return { performance, timerify, createHistogram, createSlidingWindowHistogram, monitorEventLoopDelay, importHistogram, RecordableHistogram, Histogram: RecordableHistogram, kHandle: histogramHandle, kSkipThrow: histogramBrand };
})"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let factory = context.evaluate_script_rooted(MODULE, "node:perf_hooks.js")?;
    let now = context.host_function(crate::host::shared_vm::operation("perfHooksNow"))?;
    let undefined = context.undefined();
    let data = context.null_object_rooted()?;
    let global = context.global_root()?;
    let array = super::event_constructor_shared_vm::get(context, global, "Array")?;
    let prototype = super::event_constructor_shared_vm::get(context, array, "prototype")?;
    let sort = super::event_constructor_shared_vm::get(context, prototype, "sort")?;
    let compare =
        context.host_function(crate::host::shared_vm::operation("perfHooksCompareEntries"))?;
    let coerce = context.evaluate_script_rooted(
        quench_js_check::checked_js!("value => `${value}`"),
        "node:perf_hooks/coerce.js",
    )?;
    for (name, value) in [("sort", sort), ("compare", compare), ("coerce", coerce)] {
        let key = context.string_rooted(name);
        context.define_data_property_rooted(data, key, value, false, false, false)?;
    }
    let query = context
        .host_function_with_data(crate::host::shared_vm::operation("perfHooksEntries"), data)?;
    let user_timing = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "perf_hooks.js",
        "internal/perf/usertiming.js",
    )?;
    let resource_timing = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "perf_hooks.js",
        "internal/perf/resource_timing.js",
    )?;
    let observe = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "perf_hooks.js",
        "internal/perf/observe.js",
    )?;
    let node_timing = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "perf_hooks.js",
        "internal/perf/nodetiming.js",
    )?;
    let event_loop = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "perf_hooks.js",
        "internal/perf/event_loop_utilization.js",
    )?;
    let key = context.string_rooted("eventLoopUtilization");
    let event_loop_utilization = context.get_property_rooted(event_loop, key)?;
    let performance_impl = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "perf_hooks.js",
        "internal/perf/performance.js",
    )?;
    let performance_class_key = context.string_rooted("Performance");
    let performance_class = context.get_property_rooted(performance_impl, performance_class_key)?;
    let binding = super::performance_binding_shared_vm::binding(context)?;
    let time_origin_ms = context
        .host_mut()
        .shared_state()
        .borrow()
        .process_control
        .performance_origins()
        .map(|(_, unix_micros)| unix_micros / 1_000.0)
        .ok_or_else(|| RootedError::host("Performance time origin unavailable"))?;
    let time_origin = context.number(time_origin_ms);
    let constants_key = context.string_rooted("constants");
    let constants = context.get_property_rooted(binding, constants_key)?;
    let entry_types = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "perf_hooks.js",
        "internal/perf/performance_entry.js",
    )?;
    let create_entry_key = context.string_rooted("createPerformanceNodeEntry");
    let create_entry = context.get_property_rooted(entry_types, create_entry_key)?;
    let enqueue_key = context.string_rooted("enqueue");
    let enqueue = context.get_property_rooted(observe, enqueue_key)?;
    let mut internal = Vec::new();
    for (module, name) in [
        (resource_timing, "markResourceTiming"),
        (user_timing, "mark"),
        (user_timing, "measure"),
        (user_timing, "clearMarkTimings"),
        (observe, "clearEntriesFromBuffer"),
    ] {
        let key = context.string_rooted(name);
        internal.push(context.get_property_rooted(module, key)?);
    }
    let mark_as_uncloneable =
        context.host_constructor(crate::host::shared_vm::operation("markAsUncloneable"))?;
    let module = context.call_rooted(
        factory,
        undefined,
        &[
            time_origin,
            now,
            query,
            internal[0],
            internal[1],
            internal[2],
            internal[3],
            internal[4],
            node_timing,
            event_loop_utilization,
            create_entry,
            enqueue,
            mark_as_uncloneable,
        ],
    )?;
    let performance = super::event_constructor_shared_vm::get(context, module, "performance")?;
    let timerify = super::event_constructor_shared_vm::get(context, module, "timerify")?;
    let performance_timerify = context.string_rooted("timerify");
    context.define_data_property_rooted(
        performance,
        performance_timerify,
        timerify,
        true,
        true,
        true,
    )?;
    let key = context.string_rooted("timerify");
    context.define_data_property_rooted(module, key, timerify, true, true, true)?;
    let key = context.string_rooted("Performance");
    context.define_data_property_rooted(module, key, performance_class, true, true, true)?;
    let key = context.string_rooted("constants");
    context.define_data_property_rooted(module, key, constants, true, true, true)?;
    let brand = context.symbol_rooted(Some("performance"));
    let yes = context.boolean(true);
    context.define_data_property_rooted(performance, brand, yes, true, true, true)?;
    let key = context.string_rooted("brand");
    context.define_data_property_rooted(data, key, brand, false, false, false)?;
    let resource_types = resource_timing;
    let public_types = [
        (entry_types, "PerformanceEntry"),
        (user_timing, "PerformanceMark"),
        (user_timing, "PerformanceMeasure"),
        (resource_types, "PerformanceResourceTiming"),
        (observe, "PerformanceObserver"),
        (observe, "PerformanceObserverEntryList"),
    ];
    for (source, name) in public_types {
        let key = context.string_rooted(name);
        let value = context.get_property_rooted(source, key)?;
        context.define_data_property_rooted(module, key, value, true, true, true)?;
    }
    let key = context.string_rooted("eventLoopUtilization");
    context.define_data_property_rooted(module, key, event_loop_utilization, true, true, true)?;
    Ok(module)
}

pub(crate) fn now(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let elapsed_ms = context
        .host_mut()
        .shared_state()
        .borrow()
        .process_control
        .performance_elapsed_ms();
    Ok(context.number(elapsed_ms))
}
