/**
 * Home Page
 *
 * Demonstrates for 0.1:
 * - Route handler with data
 * - Static components
 * - Array.map for lists
 */

import { PageProps, HandlerContext } from "$fresh/server.ts";

interface HomeData {
  greeting: string;
  features: string[];
}

export const handler = {
  async GET(req: Request, ctx: HandlerContext): Promise<Response> {
    const data: HomeData = {
      greeting: "Welcome to runts!",
      features: ["Fast", "Small", "Native"]
    };
    return ctx.render(data);
  }
};

export default function Home({ data }: PageProps<HomeData>) {
  return (
    <div class="home">
      <h1>{data.greeting}</h1>
      <ul>
        {data.features.map((feature, i) => (
          <li key={i}>{feature}</li>
        ))}
      </ul>
      <a href="/blog">Blog</a>
    </div>
  );
}
