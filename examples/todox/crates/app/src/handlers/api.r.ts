// api.r.ts - API handlers for task operations

import { Task, Result } from "../state.r.ts";

/**
 * Validate task data from external source.
 */
export function validateTask(task: unknown): Result<Task> {
    if (!task || typeof task !== "object") {
        return { ok: false, error: "Invalid task object" };
    }

    const t = task as Record<string, unknown>;

    if (typeof t.id !== "number") {
        return { ok: false, error: "Missing or invalid id" };
    }

    if (typeof t.title !== "string") {
        return { ok: false, error: "Missing or invalid title" };
    }

    if (typeof t.done !== "boolean") {
        return { ok: false, error: "Missing or invalid done status" };
    }

    return {
        ok: true,
        value: {
            id: t.id,
            title: t.title.trim(),
            done: t.done,
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
 * Deserialize tasks from storage.
 */
export function deserializeTasks(data: string): Result<Task[]> {
    try {
        const parsed = JSON.parse(data);
        if (!Array.isArray(parsed)) {
            return { ok: false, error: "Expected array" };
        }

        const tasks: Task[] = [];
        for (const item of parsed) {
            const result = validateTask(item);
            if (!result.ok) {
                return { ok: false, error: result.error };
            }
            tasks.push(result.value);
        }

        return { ok: true, value: tasks };
    } catch (e) {
        return { ok: false, error: "Failed to parse JSON" };
    }
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
