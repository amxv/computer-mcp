const COMPONENT = "zodex-cloudflare-worker";
const HEALTH_PATH = "/health";
const STATUS_PATH = "/status";
const MCP_ROOT_PATH = "/mcp/";
const RETRYABLE_HEALTH_STATUSES = new Set([408, 425, 429, 500, 502, 503, 504]);
const HEALTH_RETRY_DELAYS_MS = [0, 250, 750, 1500];
const WARMUP_TIMEOUTS_MS = [1500, 3000, 6000];
const UPSTREAM_RESPONSE_TIMEOUT_MS = 20000;

export default {
  async fetch(request, env) {
    return handleRequest(request, env);
  },
};

export async function handleRequest(request, env, dependencies = {}) {
  const fetchImpl = dependencies.fetch ?? fetch;
  const sleepImpl = dependencies.sleep ?? sleep;
  const url = new URL(request.url);
  const route = resolveRoute(url.pathname);

  if (route.kind === "status") {
    return workerStatus(env);
  }

  if (route.kind === "not-found") {
    return json({ error: "not_found" }, 404, env);
  }

  if (route.kind === "health") {
    return proxyHealthWithRetry(request, env, fetchImpl, sleepImpl);
  }

  await warmSprite(env, fetchImpl, sleepImpl);
  return proxyMcpOnce(request, env, route.upstreamPath, fetchImpl);
}

export function resolveRoute(pathname) {
  if (pathname === "/" || pathname === STATUS_PATH) {
    return { kind: "status" };
  }

  if (pathname === HEALTH_PATH) {
    return { kind: "health", upstreamPath: HEALTH_PATH };
  }

  if (pathname === "/mcp" || pathname === MCP_ROOT_PATH) {
    return { kind: "mcp", upstreamPath: MCP_ROOT_PATH };
  }

  if (pathname.startsWith(MCP_ROOT_PATH)) {
    return { kind: "mcp", upstreamPath: pathname };
  }

  return { kind: "not-found" };
}

function workerStatus(env) {
  return json(
    {
      ok: true,
      component: COMPONENT,
      build: env.ZODEX_WORKER_BUILD ?? "unknown",
      spriteOrigin: env.SPRITE_ORIGIN ?? null,
      routes: ["/", STATUS_PATH, HEALTH_PATH, "/mcp", MCP_ROOT_PATH],
    },
    200,
    env,
  );
}

async function proxyHealthWithRetry(request, env, fetchImpl, sleepImpl) {
  const url = new URL(request.url);
  let lastError = null;

  for (let index = 0; index < HEALTH_RETRY_DELAYS_MS.length; index += 1) {
    if (HEALTH_RETRY_DELAYS_MS[index] > 0) {
      await sleepImpl(HEALTH_RETRY_DELAYS_MS[index]);
    }

    try {
      const response = await fetchWithTimeout(
        fetchImpl,
        buildUpstreamUrl(env, HEALTH_PATH, url.search),
        {
          method: "GET",
          headers: forwardedHeaders(request),
        },
        WARMUP_TIMEOUTS_MS[Math.min(index, WARMUP_TIMEOUTS_MS.length - 1)],
      );

      if (!RETRYABLE_HEALTH_STATUSES.has(response.status)) {
        return relayResponse(response, env);
      }

      if (index + 1 === HEALTH_RETRY_DELAYS_MS.length) {
        return relayResponse(response, env);
      }
      response.body?.cancel();
    } catch (error) {
      lastError = error;
    }
  }

  return upstreamFailure(lastError, env);
}

async function warmSprite(env, fetchImpl, sleepImpl) {
  for (let index = 0; index < WARMUP_TIMEOUTS_MS.length; index += 1) {
    try {
      const response = await fetchWithTimeout(
        fetchImpl,
        buildUpstreamUrl(env, HEALTH_PATH),
        {
          method: "GET",
          headers: noCacheHeaders(),
        },
        WARMUP_TIMEOUTS_MS[index],
      );

      response.body?.cancel();

      if (response.ok) {
        return;
      }
    } catch {
      // A failed readiness probe is safe to retry because it has no side effect.
    }

    if (index + 1 < WARMUP_TIMEOUTS_MS.length) {
      await sleepImpl(250);
    }
  }
}

async function proxyMcpOnce(request, env, upstreamPath, fetchImpl) {
  const incomingUrl = new URL(request.url);

  try {
    const upstreamUrl = buildUpstreamUrl(env, upstreamPath, incomingUrl.search);
    const response = await fetchWithTimeout(
      fetchImpl,
      upstreamUrl,
      {
        method: request.method,
        headers: forwardedHeaders(request),
        body: shouldSendBody(request.method) ? request.body : undefined,
        redirect: "manual",
        duplex: shouldSendBody(request.method) ? "half" : undefined,
      },
      UPSTREAM_RESPONSE_TIMEOUT_MS,
    );

    return relayResponse(response, env);
  } catch (error) {
    // Never replay an MCP request after dispatch. The upstream may already have
    // executed a side effect even when the edge observes an ambiguous failure.
    return upstreamFailure(error, env);
  }
}

function forwardedHeaders(request) {
  const url = new URL(request.url);
  const headers = new Headers(request.headers);
  headers.delete("content-length");
  headers.delete("host");
  headers.set("x-forwarded-host", url.host);
  headers.set("x-forwarded-proto", url.protocol.replace(":", ""));
  headers.set("x-proxy-origin", "cloudflare-worker");
  return headers;
}

function relayResponse(response, env) {
  const headers = new Headers(response.headers);
  headers.set("cache-control", "no-store");
  headers.set("x-proxy-upstream", upstreamHost(env));
  headers.set("x-zodex-worker-build", env.ZODEX_WORKER_BUILD ?? "unknown");

  return new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers,
  });
}

function upstreamFailure(error, env) {
  return json(
    {
      error: "upstream_fetch_failed",
      detail: formatError(error),
    },
    502,
    env,
  );
}

function buildUpstreamUrl(env, pathname, search = "") {
  const origin = env.SPRITE_ORIGIN;
  if (!origin) {
    throw new Error("SPRITE_ORIGIN is not configured");
  }

  const url = new URL(pathname, ensureTrailingSlash(origin));
  url.search = search;
  return url;
}

function upstreamHost(env) {
  try {
    return new URL(env.SPRITE_ORIGIN).host;
  } catch {
    return "unknown";
  }
}

function ensureTrailingSlash(value) {
  return value.endsWith("/") ? value : `${value}/`;
}

function shouldSendBody(method) {
  return method !== "GET" && method !== "HEAD";
}

function noCacheHeaders() {
  return {
    "cache-control": "no-store",
    pragma: "no-cache",
  };
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function fetchWithTimeout(fetchImpl, input, init, timeoutMs) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort("timeout"), timeoutMs);

  try {
    return await fetchImpl(input, {
      ...init,
      signal: controller.signal,
      redirect: "manual",
    });
  } finally {
    clearTimeout(timer);
  }
}

function formatError(error) {
  if (error instanceof Error) {
    return error.message;
  }

  return error == null ? "unknown upstream error" : String(error);
}

function json(payload, status, env) {
  const headers = new Headers({
    "cache-control": "no-store",
    "content-type": "application/json; charset=utf-8",
  });
  headers.set("x-zodex-worker-build", env?.ZODEX_WORKER_BUILD ?? "unknown");
  return new Response(JSON.stringify(payload, null, 2), { status, headers });
}
