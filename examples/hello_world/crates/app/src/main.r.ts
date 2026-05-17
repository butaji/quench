// main.r.ts - Hello World with Rust std library usage.
// 
// Demonstrates:
// - Basic string manipulation
// - Array operations
// - Format strings
// - Pattern matching

/// Greeting message structure.
export type Greeting = {
    message: string;
    count: number;
};

/// Create a personalized greeting.
export function createGreeting(name: string): Greeting {
    return {
        message: `Hello, ${name}!`,
        count: name.length,
    };
}

/// Format a list of names.
export function formatNames(names: string[]): string {
    if (names.length === 0) {
        return "No names provided";
    }
    if (names.length === 1) {
        return names[0];
    }
    if (names.length === 2) {
        return `${names[0]} and ${names[1]}`;
    }
    const last = names[names.length - 1];
    const rest = names.slice(0, -1);
    return `${rest.join(", ")}, and ${last}`;
}

/// Process a list of numbers and return statistics.
export type Stats = {
    sum: number;
    average: number;
    count: number;
};

export function calculateStats(numbers: number[]): Stats {
    if (numbers.length === 0) {
        return { sum: 0, average: 0, count: 0 };
    }
    
    let sum = 0;
    for (let i = 0; i < numbers.length; i++) {
        sum = sum + numbers[i];
    }
    
    return {
        sum,
        average: sum / numbers.length,
        count: numbers.length,
    };
}

/// Filter and transform names based on length.
export function filterByLength(names: string[], minLength: number): string[] {
    const result: string[] = [];
    for (let i = 0; i < names.length; i++) {
        if (names[i].length >= minLength) {
            result.push(names[i]);
        }
    }
    return result;
}

/// Result pattern: validate user input.
export type ValidationResult = {
    ok: boolean;
    value?: string;
    error?: string;
};

export function validateUsername(username: string): ValidationResult {
    if (username.length === 0) {
        return { ok: false, error: "Username cannot be empty" };
    }
    if (username.length < 3) {
        return { ok: false, error: "Username must be at least 3 characters" };
    }
    if (username.length > 20) {
        return { ok: false, error: "Username must be at most 20 characters" };
    }
    return { ok: true, value: username };
}

/// Option pattern: find first match.
export function findFirstLongName(names: string[], minLength: number): string | null {
    for (let i = 0; i < names.length; i++) {
        if (names[i].length >= minLength) {
            return names[i];
        }
    }
    return null;
}

/// Enum-like pattern with tagged union.
export type Status = 
    | { tag: "pending" }
    | { tag: "active"; duration: number }
    | { tag: "completed"; timestamp: number };

export function getStatusMessage(status: Status): string {
    switch (status.tag) {
        case "pending":
            return "Operation is pending...";
        case "active":
            return `Active for ${status.duration} seconds`;
        case "completed":
            return "Operation completed";
    }
}
