# Migration Guide: Fresh → runts

This guide helps you migrate your Fresh applications to runts.

## Overview

**runts** provides framework-level compatibility with Fresh. Most Fresh code works with minimal changes.

## Key Differences

| Aspect | Fresh (Deno) | runts |
|--------|-------------|-------|
| Runtime | Deno | Native Rust |
| Server | Deno HTTP | Axum |
| JS Bundle | Full Preact | Minimal (~12KB) |
| TypeScript | Deno's TS | Custom parser |
| Build | Bytecode | Native binary |

## Step-by-Step Migration

### 1. Project Structure

Your project structure stays the same:

```
my-app/
├── routes/           ✅ Same
├── islands/         ✅ Same  
├── components/      ✅ Same
├── static/          ✅ Same
├── deno.json        → runts.config.json
└── fresh.config.ts  → Not needed
```

### 2. Configuration

**Before (deno.json):**
```json
{
  "imports": {
    "$fresh/": "https://deno.land/x/fresh@1.6.0/",
    "preact": "https://esm.sh/preact@10.19.0",
    "preact/": "https://esm.sh/preact@10.19.0/"
  },
  "tasks": {
    "start": "deno run -A --watch=static/,routes/ dev.ts",
    "build": "deno run -A dev.ts build",
    "preview": "deno run -A main.ts"
  }
}
```

**After (runts.config.json):**
```json
{
  "name": "my-app",
  "version": "0.1.0",
  "server": {
    "host": "0.0.0.0",
    "port": 8000
  },
  "routes": {
    "dir": "routes",
    "layouts": true,
    "middleware": true
  },
  "islands": {
    "dir": "islands",
    "hydration": {
      "default": "lazy"
    }
  },
  "build": {
    "release": {
      "lto": true,
      "optLevel": "z"
    }
  }
}
```

### 3. Imports

**Before:**
```typescript
import { PageProps } from "$fresh/server.ts";
import { useState } from "preact/hooks";
import { Signal, signal } from "@preact/signals";
```

**After:**
```typescript
import { PageProps } from "$fresh/server";  // Optional alias
import { useState } from "preact/hooks";
import { signal } from "@preact/signals";
```

### 4. Type Definitions

**Before:**
```typescript
// File: import_deno.ts.d.ts
declare module "$fresh/server.ts" {
  interface PageProps {
    // ...
  }
}
```

**After:**
```typescript
// Use inline types or lib/types.ts
interface MyProps extends PageProps {
  data: { title: string };
}
```

### 5. Route Handlers

**Before:**
```typescript
// routes/blog/[slug].tsx
import { PageProps } from "$fresh/server.ts";

interface Data {
  title: string;
  content: string;
}

export default function BlogPost({ params, data }: PageProps & { data: Data }) {
  return (
    <article>
      <h1>{data.title}</h1>
      <div>{data.content}</div>
    </article>
  );
}

export const handler: Handlers<Data> = {
  async GET(req, ctx) {
    const post = await getPost(ctx.params.slug);
    return ctx.render({ 
      title: post.title, 
      content: post.content 
    });
  }
};
```

**After (same!):**
```typescript
// routes/blog/[slug].tsx - works exactly the same!
import { PageProps } from "$fresh/server";

interface Data {
  title: string;
  content: string;
}

export default function BlogPost({ params, data }: PageProps & { data: Data }) {
  return (
    <article>
      <h1>{data.title}</h1>
      <div>{data.content}</div>
    </article>
  );
}

export const handler = {
  async GET(req, ctx) {
    const post = await getPost(ctx.params.slug);
    return ctx.render({ 
      title: post.title, 
      content: post.content 
    });
  }
};
```

### 6. Middleware

**Before:**
```typescript
// routes/_middleware.ts
import { FreshContext } from "$fresh/server.ts";

export default async function handler(
  req: Request,
  ctx: FreshContext,
) {
  ctx.state.requestId = crypto.randomUUID();
  return await ctx.next();
}
```

**After:**
```typescript
// routes/_middleware.ts - same code works!
import { FreshContext } from "$fresh/server";

export default async function handler(
  req: Request,
  ctx: FreshContext,
) {
  ctx.state.requestId = crypto.randomUUID();
  return await ctx.next();
}
```

### 7. Islands

Islands work exactly the same:

```typescript
// islands/Counter.tsx
import { useState } from "preact/hooks";

interface Props {
  initial?: number;
}

export default function Counter({ initial = 0 }: Props) {
  const [count, setCount] = useState(initial);
  
  return (
    <div class="counter">
      <p>Count: {count}</p>
      <button onClick={() => setCount(count + 1)}>+</button>
    </div>
  );
}
```

### 8. Static Components

Components in `components/` are rendered server-side only:

```typescript
// components/Header.tsx
export default function Header({ title }: { title: string }) {
  return (
    <header>
      <h1>{title}</h1>
      <nav>...</nav>
    </header>
  );
}
```

### 9. Signals

**Before:**
```typescript
import { signal } from "@preact/signals";

const count = signal(0);
console.log(count.value); // 0
count.value = 1;
```

**After (same!):**
```typescript
import { signal } from "@preact/signals";

const count = signal(0);
console.log(count.value); // 0
count.value = 1;
```

### 10. Common Patterns

#### Async Data Fetching

**Before & After (same):**
```typescript
export const handler = {
  async GET(req, ctx) {
    const data = await fetchData(ctx.params.id);
    return ctx.render({ data });
  }
};
```

#### Form Handling

**Before:**
```typescript
import { Handlers, PageProps } from "$fresh/server.ts";

interface FormData {
  name: string;
  email: string;
}

export const handler: Handlers<FormData> = {
  async POST(req, ctx) {
    const form = await req.formData();
    // Process form...
    return new Response("OK");
  }
};
```

**After (same):**
```typescript
// routes/api/contact.tsx
export const handler = {
  async POST(req) {
    const form = await req.formData();
    // Process form...
    return new Response("OK");
  }
};
```

## Limitations

### Not Supported

1. **`enum` declarations**
   ```typescript
   // ❌ Not supported
   enum Color { Red, Green, Blue }
   
   // ✅ Use const objects instead
   const Color = {
     Red: "red",
     Green: "green", 
     Blue: "blue"
   } as const;
   ```

2. **Class components**
   ```typescript
   // ❌ Not supported
   class MyComponent extends Component {}
   
   // ✅ Use function components
   function MyComponent() { ... }
   ```

3. **`eval()` and `new Function()`**
   - Security risk, not supported

4. **Browser-only APIs in SSR**
   ```typescript
   // ❌ Won't work in SSR
   localStorage.setItem("key", "value");
   
   // ✅ Use lib/ with runtime detection
   import { IS_BROWSER } from "$fresh/server";
   if (IS_BROWSER) {
     localStorage.setItem("key", "value");
   }
   ```

5. **Decorators**
   ```typescript
   // ❌ Not supported
   @decorator
   class MyClass {}
   
   // ✅ Use function wrappers
   function withDecorator(MyClass) { ... }
   ```

## Running Your Migrated App

### Development
```bash
# Start dev server with hot reload
runts dev

# Or with custom path
runts dev ./my-app
```

### Production
```bash
# Build native binary
runts build

# Run the binary
./target/release/my-app

# Or with Cargo
cargo run --release
```

## Troubleshooting

### "Module not found" errors
- Check your `runts.config.json` paths
- Ensure TypeScript imports use `.tsx` extension when needed

### "Hook called in wrong order"
- Hooks must be called at the top level
- Don't call hooks inside conditionals or loops

### "Cannot serialize props"
- Only JSON-serializable values can be passed to islands
- Functions, Dates, class instances need special handling

## Getting Help

- 📖 [Documentation](docs/)
- 🐛 [Issue Tracker](https://github.com/runts/runts/issues)
- 💬 [Discord](https://discord.gg/runts)

---

**Next Steps:**
1. Run `runts init` to create a new project
2. Copy your existing `routes/`, `islands/`, `components/` folders
3. Create `runts.config.json` 
4. Run `runts dev` to test
5. Fix any errors and iterate
