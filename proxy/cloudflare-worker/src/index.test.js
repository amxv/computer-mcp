import { describe, expect, test } from "bun:test";
import { handleRequest, resolveRoute } from "./index.js";

const env = {
  SPRITE_ORIGIN: "https://example.sprites.app",
  ZODEX_WORKER_BUILD: "0.1.0-testbuild",
};

function noSleep() {
  return Promise.resolve();
}

describe("routing", () => {
  test("exposes status and removes docs proxy behavior", async () => {
    expect(resolveRoute("/").kind).toBe("status");
    expect(resolveRoute("/status").kind).toBe("status");
    expect(resolveRoute("/docs").kind).toBe("not-found");

    const response = await handleRequest(
      new Request("https://worker.example/docs"),
      env,
      { fetch: () => Promise.reject(new Error("must not fetch")), sleep: noSleep },
    );
    expect(response.status).toBe(404);
  });

  test("status reports non-secret component and build identity", async () => {
    const response = await handleRequest(new Request("https://worker.example/status"), env);
    expect(response.status).toBe(200);
    expect(response.headers.get("x-zodex-worker-build")).toBe(env.ZODEX_WORKER_BUILD);
    expect(await response.json()).toEqual({
      ok: true,
      component: "zodex-cloudflare-worker",
      build: env.ZODEX_WORKER_BUILD,
      spriteOrigin: env.SPRITE_ORIGIN,
      routes: ["/", "/status", "/health", "/mcp", "/mcp/"],
    });
  });
});

describe("wake and forwarding", () => {
  test("health retries idempotent transient failures", async () => {
    const statuses = [503, 502, 200];
    const calls = [];
    const fetchImpl = async (url) => {
      calls.push(String(url));
      return new Response("ok", { status: statuses.shift() });
    };

    const response = await handleRequest(
      new Request("https://worker.example/health"),
      env,
      { fetch: fetchImpl, sleep: noSleep },
    );

    expect(response.status).toBe(200);
    expect(calls).toHaveLength(3);
    expect(calls.every((url) => url === `${env.SPRITE_ORIGIN}/health`)).toBe(true);
  });

  test("health returns the final retryable response without canceling its body", async () => {
    const fetchImpl = async () => new Response("still starting", { status: 503 });
    const response = await handleRequest(
      new Request("https://worker.example/health"),
      env,
      { fetch: fetchImpl, sleep: noSleep },
    );

    expect(response.status).toBe(503);
    expect(await response.text()).toBe("still starting");
  });

  test("dispatches an MCP request once even for a retryable upstream status", async () => {
    const calls = [];
    const fetchImpl = async (url, init) => {
      calls.push({ url: String(url), method: init?.method });
      if (String(url).endsWith("/health")) {
        return new Response("ready", { status: 200 });
      }
      return new Response("busy", { status: 503 });
    };

    const response = await handleRequest(
      new Request("https://worker.example/mcp?key=test-secret", {
        method: "POST",
        body: JSON.stringify({ jsonrpc: "2.0", method: "tools/call" }),
        headers: { "content-type": "application/json" },
      }),
      env,
      { fetch: fetchImpl, sleep: noSleep },
    );

    expect(response.status).toBe(503);
    const mcpCalls = calls.filter((call) => call.url.includes("/mcp/"));
    expect(mcpCalls).toHaveLength(1);
    expect(mcpCalls[0].url).toBe(`${env.SPRITE_ORIGIN}/mcp/?key=test-secret`);
  });

  test("does not replay MCP after an ambiguous network failure", async () => {
    let mcpCalls = 0;
    const fetchImpl = async (url) => {
      if (String(url).endsWith("/health")) {
        return new Response("ready", { status: 200 });
      }
      mcpCalls += 1;
      throw new Error("late upstream reset");
    };

    const response = await handleRequest(
      new Request("https://worker.example/mcp", { method: "POST", body: "{}" }),
      env,
      { fetch: fetchImpl, sleep: noSleep },
    );

    expect(response.status).toBe(502);
    expect(mcpCalls).toBe(1);
    expect((await response.json()).detail).toContain("late upstream reset");
  });

  test("preserves streamed upstream responses", async () => {
    const encoder = new TextEncoder();
    const stream = new ReadableStream({
      start(controller) {
        controller.enqueue(encoder.encode("one"));
        controller.enqueue(encoder.encode("two"));
        controller.close();
      },
    });
    const fetchImpl = async (url) => {
      if (String(url).endsWith("/health")) {
        return new Response("ready", { status: 200 });
      }
      return new Response(stream, { status: 200, headers: { "x-upstream": "yes" } });
    };

    const response = await handleRequest(
      new Request("https://worker.example/mcp", { method: "POST", body: "{}" }),
      env,
      { fetch: fetchImpl, sleep: noSleep },
    );

    expect(response.headers.get("x-upstream")).toBe("yes");
    expect(response.headers.get("cache-control")).toBe("no-store");
    expect(await response.text()).toBe("onetwo");
  });
});
