// main.r.ts - Data Processing Example
//
// Demonstrates data processing patterns:
// - Array transformations (map, filter, reduce)
// - Sorting and grouping
// - Aggregation pipelines
// - Serialization/deserialization

export type DataPoint = {
    timestamp: number;
    value: number;
    category: string;
};

export type ProcessedResult = {
    total: number;
    average: number;
    min: number;
    max: number;
    byCategory: Map<string, number>;
};

export type GroupedData<T> = {
    key: string;
    items: T[];
    count: number;
};

/// Calculate basic statistics.
export function calculateStats(data: DataPoint[]): ProcessedResult {
    if (data.length === 0) {
        return {
            total: 0,
            average: 0,
            min: 0,
            max: 0,
            byCategory: new Map(),
        };
    }

    let sum = 0;
    let min = data[0].value;
    let max = data[0].value;
    const byCategory = new Map<string, number>();

    for (let i = 0; i < data.length; i++) {
        const point = data[i];
        sum += point.value;
        if (point.value < min) min = point.value;
        if (point.value > max) max = point.value;

        const current = byCategory.get(point.category) || 0;
        byCategory.set(point.category, current + 1);
    }

    return {
        total: data.length,
        average: sum / data.length,
        min,
        max,
        byCategory,
    };
}

/// Filter data by value range.
export function filterByRange(
    data: DataPoint[],
    min: number,
    max: number
): DataPoint[] {
    return data.filter(p => p.value >= min && p.value <= max);
}

/// Sort data by timestamp.
export function sortByTime(data: DataPoint[], ascending: boolean): DataPoint[] {
    const sorted = [...data];
    sorted.sort((a, b) => ascending 
        ? a.timestamp - b.timestamp 
        : b.timestamp - a.timestamp
    );
    return sorted;
}

/// Group data by category.
export function groupByCategory(data: DataPoint[]): GroupedData<DataPoint>[] {
    const groups = new Map<string, DataPoint[]>();

    for (let i = 0; i < data.length; i++) {
        const point = data[i];
        if (!groups.has(point.category)) {
            groups.set(point.category, []);
        }
        groups.get(point.category)!.push(point);
    }

    const result: GroupedData<DataPoint>[] = [];
    groups.forEach((items, key) => {
        result.push({ key, items, count: items.length });
    });

    return result;
}

/// Aggregate with window function simulation.
export function movingAverage(data: number[], windowSize: number): number[] {
    if (windowSize <= 0 || data.length === 0) {
        return [];
    }

    const result: number[] = [];
    for (let i = 0; i <= data.length - windowSize; i++) {
        let sum = 0;
        for (let j = 0; j < windowSize; j++) {
            sum += data[i + j];
        }
        result.push(sum / windowSize);
    }

    return result;
}

/// Detect anomalies using standard deviation.
export function detectAnomalies(
    data: DataPoint[],
    threshold: number
): DataPoint[] {
    const values = data.map(p => p.value);
    const mean = values.reduce((a, b) => a + b, 0) / values.length;
    const variance = values.reduce((sum, v) => sum + Math.pow(v - mean, 2), 0) / values.length;
    const stdDev = Math.sqrt(variance);

    return data.filter(p => Math.abs(p.value - mean) > threshold * stdDev);
}

/// Merge overlapping time ranges.
export function mergeTimeRanges(
    ranges: { start: number; end: number }[]
): { start: number; end: number }[] {
    if (ranges.length === 0) return [];

    const sorted = [...ranges].sort((a, b) => a.start - b.start);
    const merged: { start: number; end: number }[] = [sorted[0]];

    for (let i = 1; i < sorted.length; i++) {
        const current = sorted[i];
        const last = merged[merged.length - 1];

        if (current.start <= last.end) {
            last.end = Math.max(last.end, current.end);
        } else {
            merged.push(current);
        }
    }

    return merged;
}
