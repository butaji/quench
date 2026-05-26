// routes/index.tsx - Home page with islands integration
import { PageProps, HandlerContext } from "$fresh/server.ts";
import Counter from "../islands/Counter.tsx";
import TodoList from "../islands/TodoList.tsx";
import Header from "../components/Header.tsx";

interface HomeData {
  greeting: string;
  description: string;
  features: Feature[];
  stats: Stats;
}

interface Feature {
  icon: string;
  title: string;
  description: string;
}

interface Stats {
  islandsCount: number;
  componentsCount: number;
  routesCount: number;
}

/**
 * Home page handler - demonstrates:
 * - Async data fetching
 * - Route handlers (GET)
 * - Props passing
 * - Server-side rendering
 */
export const handler = {
  async GET(_req: Request, _ctx: HandlerContext): Promise<Response> {
    // Simulate async data fetching
    const data: HomeData = {
      greeting: "Welcome to runts!",
      description: "Build lightning-fast web apps with Fresh/Preact and native Rust",
      features: [
        {
          icon: "⚡",
          title: "Lightning Fast",
          description: "Native Rust compilation means instant cold starts and blazing performance",
        },
        {
          icon: "🏝️",
          title: "Islands Architecture",
          description: "Zero JS for static content, only interactive components ship JavaScript",
        },
        {
          icon: "🔒",
          title: "Type Safe",
          description: "Full TypeScript support with compile-time type checking",
        },
        {
          icon: "🎯",
          title: "Fresh Compatible",
          description: "Write Fresh-style code and compile to native binaries",
        },
      ],
      stats: {
        islandsCount: 2,
        componentsCount: 1,
        routesCount: 4,
      },
    };

    return new Response(JSON.stringify(data), {
      headers: {
        "Content-Type": "application/json",
        "X-Runtime": "Rust",
      },
    });
  },
};

export default function Home({ data }: PageProps<HomeData>) {
  return (
    <div class="home-page">
      <Header title={data.greeting} />
      
      <main class="container">
        {/* Hero Section */}
        <section class="hero">
          <h1>{data.greeting}</h1>
          <p class="lead">{data.description}</p>
          
          <div class="cta-buttons">
            <a href="/blog" class="btn btn-primary">Read the Blog</a>
            <a href="/about" class="btn btn-secondary">Learn More</a>
          </div>
        </section>

        {/* Stats Section */}
        <section class="stats">
          <div class="stat">
            <span class="stat-value">{data.stats.islandsCount}</span>
            <span class="stat-label">Islands</span>
          </div>
          <div class="stat">
            <span class="stat-value">{data.stats.componentsCount}</span>
            <span class="stat-label">Components</span>
          </div>
          <div class="stat">
            <span class="stat-value">{data.stats.routesCount}</span>
            <span class="stat-label">Routes</span>
          </div>
        </section>

        {/* Features Section */}
        <section class="features">
          <h2>Features</h2>
          <div class="feature-grid">
            {data.features.map((feature) => (
              <div key={feature.title} class="feature-card">
                <span class="feature-icon">{feature.icon}</span>
                <h3>{feature.title}</h3>
                <p>{feature.description}</p>
              </div>
            ))}
          </div>
        </section>

        {/* Interactive Demo Section */}
        <section class="demo">
          <h2>Interactive Demo</h2>
          <p>Try out the islands below - notice how only the interactive parts load JavaScript!</p>
          
          <div class="demo-grid">
            {/* Counter Island - demonstrates useState */}
            <div class="demo-item">
              <h3>Counter Island</h3>
              <Counter initial={5} step={1} label="Try clicking me!" />
              <p class="demo-note">
                This island uses <code>useState</code> for reactive state.
              </p>
            </div>
            
            {/* TodoList Island - demonstrates complex state */}
            <div class="demo-item">
              <h3>Todo List Island</h3>
              <TodoList 
                maxTodos={10} 
                title="My Tasks"
                initialTodos={[
                  {
                    id: "demo-1",
                    text: "Learn about islands",
                    completed: true,
                    createdAt: new Date(),
                  },
                  {
                    id: "demo-2",
                    text: "Build something awesome",
                    completed: false,
                    createdAt: new Date(),
                  },
                ]}
              />
              <p class="demo-note">
                This island demonstrates complex state with filtering and persistence.
              </p>
            </div>
          </div>
        </section>

        {/* Architecture Diagram */}
        <section class="architecture">
          <h2>How It Works</h2>
          <div class="arch-diagram">
            <div class="arch-step">
              <span class="arch-number">1</span>
              <h4>Write TSX</h4>
              <p>Write Fresh/Preact components in TypeScript</p>
            </div>
            <div class="arch-arrow">→</div>
            <div class="arch-step">
              <span class="arch-number">2</span>
              <h4>Compile to Rust</h4>
              <p>runts transpiles TSX to optimized Rust code</p>
            </div>
            <div class="arch-arrow">→</div>
            <div class="arch-step">
              <span class="arch-number">3</span>
              <h4>Build Binary</h4>
              <p>Rust compiles to a single native binary</p>
            </div>
            <div class="arch-arrow">→</div>
            <div class="arch-step">
              <span class="arch-number">4</span>
              <h4>Deploy</h4>
              <p>Ship a tiny, fast, secure binary anywhere</p>
            </div>
          </div>
        </section>
      </main>
      
      <footer class="footer">
        <p>Built with runts ⚡ Rust</p>
      </footer>
    </div>
  );
}
