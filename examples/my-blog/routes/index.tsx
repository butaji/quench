// routes/index.tsx - Home page with islands integration
import { PageProps } from "$fresh/server.ts";
import Counter from "../islands/Counter.tsx";
import Header from "../components/Header.tsx";

interface HomeData {
  greeting: string;
  description: string;
  version: string;
}

export const handler = {
  async GET(_req: Request, _ctx: HandlerContext): Promise<Response> {
    const data: HomeData = {
      greeting: "Welcome to runts!",
      description: "Build lightning-fast web apps with Fresh/Preact and native Rust",
      version: "0.2.0",
    };

    return new Response(JSON.stringify(data), {
      headers: {
        "Content-Type": "application/json",
        "X-Runtime": "Rust",
      },
    });
  }
};

export default function Home({ data }: PageProps<HomeData>) {
  return (
    <div class="home-page">
      <Header title={data.greeting} />
      
      <main class="container">
        <section class="hero">
          <h1>{data.greeting}</h1>
          <p class="lead">{data.description}</p>
          <p class="version">Version: {data.version}</p>
          
          <div class="cta-buttons">
            <a href="/blog" class="btn btn-primary">Read the Blog</a>
            <a href="/about" class="btn btn-secondary">Learn More</a>
          </div>
        </section>

        <section class="demo">
          <h2>Interactive Demo</h2>
          <p>Try out the counter island below!</p>
          
          <div class="demo-item">
            <h3>Counter Island</h3>
            <Counter initial={5} step={1} label="Try clicking me!" />
          </div>
        </section>
      </main>
      
      <footer class="footer">
        <p>Built with runts and Rust</p>
      </footer>
    </div>
  );
}
