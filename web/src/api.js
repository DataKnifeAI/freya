import { isMockMode, mockApi } from "./mockApi.js";

export { isMockMode };

/** Fired when a live API call returns 401 — UI should show Sign in. */
export const AUTH_REQUIRED_EVENT = "freya:auth-required";

function apiAuthHeaders() {
  const token =
    (typeof import.meta !== "undefined" && import.meta.env?.VITE_FREYA_API_TOKEN) ||
    (typeof localStorage !== "undefined" && localStorage.getItem("freya.apiToken")) ||
    "";
  if (!token) return {};
  return { Authorization: `Bearer ${token}` };
}

export function hasBrowserBearer() {
  const token =
    (typeof import.meta !== "undefined" && import.meta.env?.VITE_FREYA_API_TOKEN) ||
    (typeof localStorage !== "undefined" && localStorage.getItem("freya.apiToken")) ||
    "";
  return Boolean(String(token || "").trim());
}

export async function api(path, opts) {
  if (isMockMode()) {
    const data = await mockApi(path, opts);
    if (data && data.__redirect) {
      return data;
    }
    return data;
  }

  // Pass opts.signal through so callers can AbortController-cancel (e.g. Civitai search).
  // GET lists (worker checkpoints/LoRAs, /v1/models) must not stick in the HTTP cache
  // after an install or delete. Callers can still override `cache`.
  const method = String(opts?.method || "GET").toUpperCase();
  const res = await fetch(path, {
    cache: method === "GET" || method === "HEAD" ? "no-store" : undefined,
    ...opts,
    headers: {
      "Content-Type": "application/json",
      ...apiAuthHeaders(),
      ...(opts?.headers || {}),
    },
  });

  // Image endpoints may return binary — not used by App today (uses URL strings).
  const ct = res.headers.get("content-type") || "";
  if (ct.startsWith("image/")) {
    const blob = await res.blob();
    return { __redirect: URL.createObjectURL(blob) };
  }

  const data = await res.json().catch(() => ({}));
  if (!res.ok) {
    if (res.status === 401) {
      if (typeof window !== "undefined") {
        window.dispatchEvent(
          new CustomEvent(AUTH_REQUIRED_EVENT, {
            detail: { path, message: data.error || "Unauthorized" },
          }),
        );
      }
      throw new Error("Unauthorized (401) — Sign in with admin username and password.");
    }
    throw new Error(data.error || res.statusText || `HTTP ${res.status}`);
  }
  return data;
}

/** Chat turn. Live API streams status lines, then one result object. */
export async function postChat(body, onStatus) {
  if (isMockMode()) {
    onStatus?.("Thinking");
    const msgs = Array.isArray(body?.messages) ? body.messages : [];
    if (!body?.confirm && msgs.length === 0) onStatus?.("Searching");
    return mockApi("/v1/chat", { method: "POST", body: JSON.stringify(body) });
  }

  const res = await fetch("/v1/chat", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Accept: "application/x-ndjson",
      ...apiAuthHeaders(),
    },
    body: JSON.stringify({ ...body, stream: true }),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    if (res.status === 401) {
      if (typeof window !== "undefined") {
        window.dispatchEvent(
          new CustomEvent(AUTH_REQUIRED_EVENT, {
            detail: { path: "/v1/chat", message: data.error || "Unauthorized" },
          }),
        );
      }
      throw new Error("Unauthorized (401) — Sign in with admin username and password.");
    }
    throw new Error(data.error || res.statusText || `HTTP ${res.status}`);
  }
  const reader = res.body?.getReader();
  if (!reader) {
    const data = await res.json();
    return data;
  }
  const decoder = new TextDecoder();
  let buf = "";
  let result = null;
  while (true) {
    const { done, value } = await reader.read();
    if (done) {
      buf += decoder.decode();
      break;
    }
    buf += decoder.decode(value, { stream: true });
    let idx;
    while ((idx = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, idx).trim();
      buf = buf.slice(idx + 1);
      if (!line) continue;
      let msg;
      try {
        msg = JSON.parse(line);
      } catch {
        continue;
      }
      if (msg.type === "status" && msg.status) onStatus?.(msg.status);
      else if (msg.type === "result") result = msg.data;
      else if (msg.type === "error") throw new Error(msg.error || "Chat failed");
    }
  }
  const tail = buf.trim();
  if (tail) {
    try {
      const msg = JSON.parse(tail);
      if (msg.type === "status" && msg.status) onStatus?.(msg.status);
      else if (msg.type === "result") result = msg.data;
      else if (msg.type === "error") throw new Error(msg.error || "Chat failed");
    } catch (err) {
      if (err instanceof SyntaxError) {
        /* trailing partial chunk */
      } else {
        throw err;
      }
    }
  }
  if (!result) throw new Error("Chat ended without a reply");
  return result;
}
