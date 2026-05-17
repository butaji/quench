// main.r.ts - Rust Standard Library Usage Example
//
// Demonstrates using Rust's std library features from Rune:
// - String operations (String, &str)
// - Vec and array methods
// - HashMap/HashSet via standard patterns
// - Option and Result patterns
// - Iterators and functional patterns
// - chrono for date/time
// - uuid for unique identifiers

import { DataEntry, AppState } from "protocol";

/// Data entry type.
export type Entry = {
    id: string;
    name: string;
    value: number;
    timestamp: number;
};

/// Search result type.
export type SearchResult = {
    matches: Entry[];
    count: number;
};

/// Filter criteria for entries.
export type FilterCriteria = {
    minValue?: number;
    maxValue?: number;
    namePattern?: string;
};

/// Create a new entry with timestamp.
export function createEntry(name: string, value: number): Entry {
    return {
        id: generateUuid(),
        name,
        value,
        timestamp: currentTimestamp(),
    };
}

/// Generate UUID-like string.
export function generateUuid(): string {
    // Simplified UUID generation
    const hex = "0123456789abcdef";
    let result = "";
    for (let i = 0; i < 36; i++) {
        if (i === 8 || i === 13 || i === 18 || i === 23) {
            result += "-";
        } else if (i === 14) {
            result += "4";
        } else if (i === 19) {
            result += hex[(Math.floor(Math.random() * 4) + 8)];
        } else {
            result += hex[Math.floor(Math.random() * 16)];
        }
    }
    return result;
}

/// Get current Unix timestamp.
export function currentTimestamp(): number {
    return Math.floor(Date.now() / 1000);
}

/// Format timestamp as date string.
export function formatTimestamp(ts: number): string {
    const date = new Date(ts * 1000);
    const year = date.getFullYear();
    const month = String(date.getMonth() + 1).padStart(2, "0");
    const day = String(date.getDate()).padStart(2, "0");
    return `${year}-${month}-${day}`;
}

/// String manipulation examples.
export function processStrings(input: string[]): {
    joined: string;
    upper: string;
    words: number;
} {
    const joined = input.join(" | ");
    const upper = joined.toUpperCase();
    const words = countWords(joined);
    return { joined, upper, words };
}

function countWords(text: string): number {
    let count = 0;
    let inWord = false;
    for (let i = 0; i < text.length; i++) {
        const c = text.charAt(i);
        if (c === " " || c === "\n" || c === "\t") {
            inWord = false;
        } else if (!inWord) {
            inWord = true;
            count++;
        }
    }
    return count;
}

/// Search entries by name pattern.
export function searchEntries(entries: Entry[], query: string): SearchResult {
    if (query.length === 0) {
        return { matches: entries, count: entries.length };
    }

    const lowerQuery = query.toLowerCase();
    const matches: Entry[] = [];

    for (let i = 0; i < entries.length; i++) {
        const entry = entries[i];
        if (entry.name.toLowerCase().includes(lowerQuery)) {
            matches.push(entry);
        }
    }

    return { matches, count: matches.length };
}

/// Filter entries by value range.
export function filterByValue(
    entries: Entry[],
    min?: number,
    max?: number
): Entry[] {
    return entries.filter(e => {
        if (min !== undefined && e.value < min) return false;
        if (max !== undefined && e.value > max) return false;
        return true;
    });
}

/// Sort entries by value.
export function sortByValue(entries: Entry[], ascending: boolean): Entry[] {
    const sorted = [...entries];
    sorted.sort((a, b) => ascending 
        ? a.value - b.value 
        : b.value - a.value
    );
    return sorted;
}

/// Aggregate statistics for entries.
export function computeStats(entries: Entry[]): {
    sum: number;
    avg: number;
    min: number;
    max: number;
} {
    if (entries.length === 0) {
        return { sum: 0, avg: 0, min: 0, max: 0 };
    }

    let sum = 0;
    let min = entries[0].value;
    let max = entries[0].value;

    for (let i = 0; i < entries.length; i++) {
        const v = entries[i].value;
        sum += v;
        if (v < min) min = v;
        if (v > max) max = v;
    }

    return {
        sum,
        avg: sum / entries.length,
        min,
        max,
    };
}

/// Chunk array into smaller arrays.
export function chunkArray<T>(arr: T[], size: number): T[][] {
    if (size <= 0) return [];
    const chunks: T[][] = [];
    for (let i = 0; i < arr.length; i += size) {
        chunks.push(arr.slice(i, i + size));
    }
    return chunks;
}

/// Remove duplicates by id.
export function uniqueById(entries: Entry[]): Entry[] {
    const seen = new Set<string>();
    return entries.filter(e => {
        if (seen.has(e.id)) return false;
        seen.add(e.id);
        return true;
    });
}

/// Merge two entry lists.
export function mergeEntries(a: Entry[], b: Entry[]): Entry[] {
    return uniqueById([...a, ...b]);
}

/// Parse a value that might be null.
export function parseValue(input: string): number | null {
    const parsed = parseFloat(input);
    return isNaN(parsed) ? null : parsed;
}

/// Validate entry data.
export function validateEntry(entry: Partial<Entry>): 
    | { ok: true, value: Entry }
    | { ok: false, error: string }
{
    if (!entry.name || entry.name.length === 0) {
        return { ok: false, error: "Name is required" };
    }
    if (entry.name.length > 100) {
        return { ok: false, error: "Name too long" };
    }
    if (entry.value === undefined) {
        return { ok: false, error: "Value is required" };
    }
    if (entry.value < 0) {
        return { ok: false, error: "Value cannot be negative" };
    }

    return {
        ok: true,
        value: {
            id: entry.id || generateUuid(),
            name: entry.name,
            value: entry.value,
            timestamp: entry.timestamp || currentTimestamp(),
        },
    };
}
