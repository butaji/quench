/**
 * About Page
 *
 * Static page with conditional rendering example.
 */

import { PageProps, HandlerContext } from "$fresh/server.ts";

interface AboutData {
  title: string;
  description: string;
  features: string[];
  isStable: boolean;
}

export const handler = {
  async GET(req: Request, ctx: HandlerContext): Promise<Response> {
    const data: AboutData = {
      title: "About runts",
      description: "Static blog example for runts 0.1",
      features: [
        "File-based routing",
        "Static components",
        "Layouts with children",
        "Array.map lists",
        "Conditional rendering"
      ],
      isStable: false
    };
    return ctx.render(data);
  }
};

export default function About({ data }: PageProps<AboutData>) {
  return (
    <div class="about-page">
      <h1>{data.title}</h1>
      <p>{data.description}</p>

      <h2>Features</h2>
      <ul>
        {data.features.map((feature, i) => (
          <li key={i}>{feature}</li>
        ))}
      </ul>

      <div class="status">
        <p>Status: {data.isStable ? "Stable" : "Beta"}</p>
      </div>

      <nav>
        <a href="/">Home</a>
        <a href="/blog">Blog</a>
      </nav>
    </div>
  );
}
