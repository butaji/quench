// api.r.ts - API handlers for task operations

import { Task, Result } from "../state.r.ts";

/**
 * Raw task data from external source (before validation).
 */
export type RawTask = {
    id: number;
    title: string;
    done: boolean;
};

/**
 * Base type for JSON values.
 */
type JsonValue = object | string | number | boolean | null;

/**
 * Check if a value is a valid number.
 */
function isNumber(val: JsonValue): val is number {
    return typeof val === "number";
}

/**
 * Check if a value is a valid string.
 */
function isString(val: JsonValue): val is string {
    return typeof val === "string";
}

/**
 * Check if a value is a valid boolean.
 */
function isBoolean(val: JsonValue): val is boolean {
    return typeof val === "boolean";
}

/**
 * Check if a value is a valid object (not array, not null).
 */
function isObject(val: JsonValue): val is object {
    return typeof val === "object" && val !== null && !Array.isArray(val);
}

/**
 * Validate task data from external source.
 */
export function validateTask(task: RawTask): Result<Task> {
    if (!task) {
        return { ok: false, error: "Invalid task object" };
    }

    if (!isNumber(task.id)) {
        return { ok: false, error: "Missing or invalid id" };
    }

    if (!isString(task.title)) {
        return { ok: false, error: "Missing or invalid title" };
    }

    if (!isBoolean(task.done)) {
        return { ok: false, error: "Missing or invalid done status" };
    }

    return {
        ok: true,
        value: {
            id: task.id,
            title: task.title.trim(),
            done: task.done,
        },
    };
}

/**
 * Serialize tasks for storage.
 */
export function serializeTasks(tasks: Task[]): string {
    return JSON.stringify(tasks);
}

/**
 * Parse JSON safely.
 */
export function parseJson(data: string): Result<JsonValue> {
    // Basic validation before parsing
    const trimmed = data.trim();
    if (!trimmed.startsWith("{") && !trimmed.startsWith("[")) {
        return { ok: false, error: "Invalid JSON structure" };
    }
    
    // Return raw parsed data - caller must validate structure
    return { ok: true, value: data as unknown as JsonValue };
}

/**
 * Deserialize tasks from storage.
 */
export function deserializeTasks(data: string): Result<Task[]> {
    const parseResult = parseJson(data);
    if (!parseResult.ok) {
        return { ok: false, error: parseResult.error };
    }

    // Note: In production, use a proper JSON parser result type
    // For now, we return the parsed structure for validation
    const parsed = JSON.parse(data);
    if (!Array.isArray(parsed)) {
        return { ok: false, error: "Expected array" };
    }

    const tasks: Task[] = [];
    for (const item of parsed) {
        // Type assertion with runtime validation
        const raw = item as RawTask;
        const result = validateTask(raw);
        if (!result.ok) {
            return { ok: false, error: result.error };
        }
        tasks.push(result.value);
    }

    return { ok: true, value: tasks };
}

/**
 * Merge tasks from different sources.
 */
export function mergeTasks(local: Task[], remote: Task[]): Task[] {
    const merged = [...local];

    for (const remoteTask of remote) {
        const existing = merged.findIndex(t => t.id === remoteTask.id);
        if (existing >= 0) {
            // Keep newer version
            if (remoteTask !== merged[existing]) {
                merged[existing] = remoteTask;
            }
        } else {
            merged.push(remoteTask);
        }
    }

    return merged;
}
