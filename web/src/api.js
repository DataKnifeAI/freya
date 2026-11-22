import { isMockMode, mockApi } from "./mockApi.js";

export { isMockMode };

function apiAuthHeaders() {
  const token =
    (typeof import.meta !== "undefined" && import.meta.env?.VITE_FREYA_API_TOKEN) ||
    (typeof localStorage !== "undefined" && localStorage.getItem("freya.apiToken")) ||
    "";
  if (!token) return {};
  return { Authorization: `Bearer ${token}` };
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
  const res = await fetch(path, {
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
  if (!res.ok) throw new Error(data.error || res.statusText);
  return data;
}
