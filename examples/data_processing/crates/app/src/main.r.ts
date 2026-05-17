// main.r.ts - Data Processing Demo
// Demonstrates: arrays, filtering, mapping, generics, closures

export type Person = {
    id: number;
    name: string;
    age: number;
    salary: number;
};

export type Filter = {
    minAge?: number;
    maxAge?: number;
    minSalary?: number;
};

// Generic first function
export function first<T>(arr: T[]): T | null {
    if (arr.length > 0) {
        return arr[0];
    }
    return null;
}

// Generic last function
export function last<T>(arr: T[]): T | null {
    if (arr.length > 0) {
        return arr[arr.length - 1];
    }
    return null;
}

// Generic map function
export function map<T, U>(arr: T[], transform: (item: T) => U): U[] {
    const result: U[] = [];
    for (let i = 0; i < arr.length; i++) {
        result.push(transform(arr[i]));
    }
    return result;
}

// Generic filter function
export function filter<T>(arr: T[], predicate: (item: T) => boolean): T[] {
    const result: T[] = [];
    for (let i = 0; i < arr.length; i++) {
        if (predicate(arr[i])) {
            result.push(arr[i]);
        }
    }
    return result;
}

// Reduce function
export function reduce<T>(arr: T[], reducer: (acc: number, item: T) => number, initial: number): number {
    let acc = initial;
    for (let i = 0; i < arr.length; i++) {
        acc = reducer(acc, arr[i]);
    }
    return acc;
}

// Filter people by criteria
export function filterPeople(people: Person[], criteria: Filter): Person[] {
    return filter(people, (p) => {
        if (criteria.minAge !== undefined && p.age < criteria.minAge) {
            return false;
        }
        if (criteria.maxAge !== undefined && p.age > criteria.maxAge) {
            return false;
        }
        if (criteria.minSalary !== undefined && p.salary < criteria.minSalary) {
            return false;
        }
        return true;
    });
}

// Calculate statistics
export type Stats = {
    count: number;
    sum: number;
    average: number;
    min: number;
    max: number;
};

export function calculateStats(values: number[]): Stats {
    if (values.length === 0) {
        return { count: 0, sum: 0, average: 0, min: 0, max: 0 };
    }
    
    let sum = 0;
    let min = values[0];
    let max = values[0];
    
    for (let i = 0; i < values.length; i++) {
        sum = sum + values[i];
        if (values[i] < min) {
            min = values[i];
        }
        if (values[i] > max) {
            max = values[i];
        }
    }
    
    return {
        count: values.length,
        sum,
        average: sum / values.length,
        min,
        max,
    };
}

// Group by property
export function groupBy<T>(arr: T[], key: (item: T) => string): Record<string, T[]> {
    const result: Record<string, T[]> = {};
    
    for (let i = 0; i < arr.length; i++) {
        const item = arr[i];
        const groupKey = key(item);
        if (!result[groupKey]) {
            result[groupKey] = [];
        }
        result[groupKey].push(item);
    }
    
    return result;
}

// Sort by property
export function sortBy<T>(arr: T[], key: (item: T) => number): T[] {
    const sorted = arr.slice();
    // Bubble sort for simplicity
    for (let i = 0; i < sorted.length; i++) {
        for (let j = 0; j < sorted.length - i - 1; j++) {
            if (key(sorted[j]) > key(sorted[j + 1])) {
                const temp = sorted[j];
                sorted[j] = sorted[j + 1];
                sorted[j + 1] = temp;
            }
        }
    }
    return sorted;
}
