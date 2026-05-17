// main.r.ts - Data processing with Rust collections.
// 
// Demonstrates:
// - Vec and String operations
// - HashMap patterns
// - Iterator-style processing
// - Result/Option patterns

/// A data record.
export type Record = {
    id: number;
    name: string;
    value: number;
    tags: string[];
};

/// Process records and compute statistics.
export function computeStats(records: Record[]): RecordStats {
    if (records.length === 0) {
        return {
            count: 0,
            sum: 0,
            min: 0,
            max: 0,
            average: 0,
        };
    }
    
    let sum = 0;
    let min = records[0].value;
    let max = records[0].value;
    
    for (let i = 0; i < records.length; i++) {
        const val = records[i].value;
        sum = sum + val;
        if (val < min) { min = val; }
        if (val > max) { max = val; }
    }
    
    return {
        count: records.length,
        sum,
        min,
        max,
        average: sum / records.length,
    };
}

export type RecordStats = {
    count: number;
    sum: number;
    min: number;
    max: number;
    average: number;
};

/// Group records by tag.
export function groupByTag(records: Record[]): Map<string, Record[]> {
    const groups: Map<string, Record[]> = new Map();
    
    for (let i = 0; i < records.length; i++) {
        const record = records[i];
        for (let j = 0; j < record.tags.length; j++) {
            const tag = record.tags[j];
            if (!groups.has(tag)) {
                groups.set(tag, []);
            }
            groups.get(tag)!.push(record);
        }
    }
    
    return groups;
}

/// Find records matching predicate.
export function filterRecords(
    records: Record[], 
    predicate: (r: Record) => boolean
): Record[] {
    const result: Record[] = [];
    for (let i = 0; i < records.length; i++) {
        if (predicate(records[i])) {
            result.push(records[i]);
        }
    }
    return result;
}

/// Sort records by value.
export function sortByValue(records: Record[]): Record[] {
    const sorted = records.slice();
    for (let i = 0; i < sorted.length; i++) {
        for (let j = i + 1; j < sorted.length; j++) {
            if (sorted[j].value < sorted[i].value) {
                const temp = sorted[i];
                sorted[i] = sorted[j];
                sorted[j] = temp;
            }
        }
    }
    return sorted;
}

/// Merge two record lists, removing duplicates by ID.
export function mergeRecords(a: Record[], b: Record[]): Record[] {
    const seen = new Set<number>();
    const result: Record[] = [];
    
    for (let i = 0; i < a.length; i++) {
        if (!seen.has(a[i].id)) {
            seen.add(a[i].id);
            result.push(a[i]);
        }
    }
    
    for (let i = 0; i < b.length; i++) {
        if (!seen.has(b[i].id)) {
            seen.add(b[i].id);
            result.push(b[i]);
        }
    }
    
    return result;
}

/// Aggregate values by category.
export function aggregateByCategory(
    records: Record[],
    categories: string[]
): Map<string, number> {
    const totals = new Map<string, number>();
    
    for (let i = 0; i < categories.length; i++) {
        totals.set(categories[i], 0);
    }
    
    for (let i = 0; i < records.length; i++) {
        const record = records[i];
        for (let j = 0; j < record.tags.length; j++) {
            const tag = record.tags[j];
            if (totals.has(tag)) {
                totals.set(tag, totals.get(tag)! + record.value);
            }
        }
    }
    
    return totals;
}
