// main.r.ts - Async HTTP Demo
// Demonstrates: async/await, Promises, Result patterns, generics

export type HttpResponse<T> = {
    ok: boolean;
    status: number;
    data?: T;
    error?: string;
};

export type User = {
    id: number;
    name: string;
    email: string;
};

export type Post = {
    id: number;
    userId: number;
    title: string;
    body: string;
};

// Simulated API calls (in real use, would call native HTTP library)

// Get user by ID
export async function getUser(id: number): Promise<HttpResponse<User>> {
    // Simulate API call
    const user: User = {
        id,
        name: "User " + id,
        email: "user" + id + "@example.com",
    };
    return { ok: true, status: 200, data: user };
}

// Get posts for a user
export async function getUserPosts(userId: number): Promise<HttpResponse<Post[]>> {
    const posts: Post[] = [
        { id: 1, userId, title: "First Post", body: "Content here..." },
        { id: 2, userId, title: "Second Post", body: "More content..." },
    ];
    return { ok: true, status: 200, data: posts };
}

// Fetch multiple resources in parallel
export async function fetchUserWithPosts(userId: number): Promise<HttpResponse<{
    user: User;
    posts: Post[];
}>> {
    const userResult = await getUser(userId);
    if (!userResult.ok) {
        return { ok: false, status: userResult.status, error: userResult.error };
    }

    const postsResult = await getUserPosts(userId);
    if (!postsResult.ok) {
        return { ok: false, status: postsResult.status, error: postsResult.error };
    }

    return {
        ok: true,
        status: 200,
        data: {
            user: userResult.data!,
            posts: postsResult.data!,
        },
    };
}

// Retry a failing operation
export async function withRetry<T>(
    operation: () => Promise<HttpResponse<T>>,
    maxRetries: number
): Promise<HttpResponse<T>> {
    for (let attempt = 0; attempt < maxRetries; attempt++) {
        const result = await operation();
        if (result.ok) {
            return result;
        }
        // Could add exponential backoff here
    }
    return { ok: false, status: 500, error: "Max retries exceeded" };
}

// Map over array with async function
export async function mapAsync<T, U>(
    items: T[],
    transform: (item: T) => Promise<U>
): Promise<U[]> {
    const results: U[] = [];
    for (let i = 0; i < items.length; i++) {
        results.push(await transform(items[i]));
    }
    return results;
}

// Process users concurrently with limit
export async function processUsersConcurrently(
    userIds: number[],
    limit: number
): Promise<HttpResponse<User[][]>> {
    const chunks: number[][] = [];
    
    for (let i = 0; i < userIds.length; i += limit) {
        chunks.push(userIds.slice(i, i + limit));
    }

    const results: User[][] = [];
    for (let i = 0; i < chunks.length; i++) {
        const chunk = chunks[i];
        const users: User[] = [];
        for (let j = 0; j < chunk.length; j++) {
            const result = await getUser(chunk[j]);
            if (result.ok && result.data) {
                users.push(result.data);
            }
        }
        results.push(users);
    }

    return { ok: true, status: 200, data: results };
}
