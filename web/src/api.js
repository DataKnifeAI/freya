import { isMockMode, mockApi } from "./mockApi.js";

export { isMockMode };

export async function api(path, opts) {
  if (isMockMode()) {
    const data = await mockApi(path, opts);
    if (data && data.__redirect) {
      return data;
    }
    return data;
  }

  const res = await fetch(path, {
    headers: { "Content-Type": "application/json", ...(opts?.headers || {}) },
    ...opts,
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
