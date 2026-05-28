// ============================================================================
// ASYNC/AWAIT
// ============================================================================

// Simulating async operations (in real runtime these would be actual async)
const delay = (ms: number): Promise<void> => 
  new Promise(resolve => setTimeout(resolve, ms));

const fetchData = (id: number): Promise<{ id: number; name: string }> =>
  Promise.resolve({ id, name: `Item ${id}` });

// Basic async function
async function getData(): Promise<string> {
  return "data";
}

// Async with await
async function fetchUser(): Promise<{ name: string; age: number }> {
  await delay(100);
  return { name: "Alice", age: 30 };
}

// Async arrow function
const getUser = async (id: number): Promise<string> => {
  const user = await fetchData(id);
  return user.name;
};

// Multiple awaits
async function fetchAll(): Promise<string[]> {
  const a = await fetchData(1);
  const b = await fetchData(2);
  const c = await fetchData(3);
  return [a.name, b.name, c.name];
}

// Parallel fetching with Promise.all
async function fetchParallel(): Promise<string[]> {
  const promises = [1, 2, 3].map(id => fetchData(id));
  const results = await Promise.all(promises);
  return results.map(r => r.name);
}

// Promise.all with error handling
async function fetchWithAll() {
  try {
    const [a, b] = await Promise.all([
      Promise.resolve(1),
      Promise.resolve(2)
    ]);
    return a + b;
  } catch (e) {
    return 0;
  }
}

// Async for...of loop
async function processItems(): Promise<number[]> {
  const results: number[] = [];
  for (const id of [1, 2, 3]) {
    const item = await fetchData(id);
    results.push(item.id);
  }
  return results;
}

// Sequential await in loop (not parallel)
async function sequential(): Promise<number[]> {
  const nums: number[] = [];
  for (let i = 0; i < 3; i++) {
    await delay(10);
    nums.push(i);
  }
  return nums;
}

// Async with try/catch
async function safeFetch(id: number): Promise<string> {
  try {
    const data = await fetchData(id);
    return data.name;
  } catch (error) {
    return "Error";
  }
}

// Returning promises from async
async function getNumbers(): Promise<number[]> {
  return [1, 2, 3]; // implicitly wrapped in Promise.resolve
}

// Async method in class
class ApiService {
  async getUser(id: number): Promise<{ id: number; name: string }> {
    await delay(100);
    return { id, name: `User ${id}` };
  }

  async getUsers(): Promise<{ id: number; name: string }[]> {
    return Promise.all([1, 2, 3].map(id => this.getUser(id)));
  }
}
