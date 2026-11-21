import { useEffect, useState } from "react";

const api = (path, opts) =>
  fetch(path, {
    headers: { "Content-Type": "application/json", ...(opts?.headers || {}) },
    ...opts,
  }).then(async (r) => {
    const data = await r.json().catch(() => ({}));
    if (!r.ok) throw new Error(data.error || r.statusText);
    return data;
  });

export default function App() {
  const [tab, setTab] = useState("compose");
  const [prompt, setPrompt] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [result, setResult] = useState(null);
  const [styles, setStyles] = useState([]);
  const [gallery, setGallery] = useState([]);
  const [settings, setSettings] = useState(null);
  const [civitaiKey, setCivitaiKey] = useState("");
  const [openrouterKey, setOpenrouterKey] = useState("");
  const [workerUrl, setWorkerUrl] = useState("");
  const [civitaiQ, setCivitaiQ] = useState("anime lora");
  const [civitaiItems, setCivitaiItems] = useState([]);
  const [savedMsg, setSavedMsg] = useState("");

  const refresh = async () => {
    try {
      const [s, st, g] = await Promise.all([
        api("/v1/styles"),
        api("/v1/settings"),
        api("/v1/gallery"),
      ]);
      setStyles(s.items || []);
      setSettings(st);
      setWorkerUrl(st.diffusionWorkerUrl || "");
      setGallery(g.items || []);
    } catch (e) {
      setError(e.message);
    }
  };

  useEffect(() => {
    refresh();
  }, []);

  const run = async () => {
    setBusy(true);
    setError("");
    try {
      const data = await api("/v1/run", {
        method: "POST",
        body: JSON.stringify({ prompt, enhance: "auto", backend: "auto" }),
      });
      setResult(data);
      const g = await api("/v1/gallery");
      setGallery(g.items || []);
    } catch (e) {
      setError(e.message);
    } finally {
      setBusy(false);
    }
  };

  const enhanceOnly = async () => {
    setBusy(true);
    setError("");
    try {
      const data = await api("/v1/assist", {
        method: "POST",
        body: JSON.stringify({ mode: "prompt-prose", message: prompt }),
      });
      setPrompt(data.text || prompt);
    } catch (e) {
      setError(e.message);
    } finally {
      setBusy(false);
    }
  };

  const saveSettings = async () => {
    setBusy(true);
    setError("");
    setSavedMsg("");
    try {
      const body = { diffusionWorkerUrl: workerUrl };
      if (civitaiKey.trim()) body.civitaiApiKey = civitaiKey.trim();
      if (openrouterKey.trim()) body.openrouterApiKey = openrouterKey.trim();
      const st = await api("/v1/settings", {
        method: "PUT",
        body: JSON.stringify(body),
      });
      setSettings(st);
      setCivitaiKey("");
      setOpenrouterKey("");
      setSavedMsg("Settings saved. Keys are stored for this Freya API instance.");
    } catch (e) {
      setError(e.message);
    } finally {
      setBusy(false);
    }
  };

  const searchCivitai = async () => {
    setBusy(true);
    setError("");
    try {
      const data = await api(`/v1/civitai/search?q=${encodeURIComponent(civitaiQ)}&limit=12`);
      setCivitaiItems(data.items || []);
    } catch (e) {
      setError(e.message);
    } finally {
      setBusy(false);
    }
  };

  const installCivitai = async (item) => {
    if (!item.downloadUrl) {
      setError("No download URL on this card");
      return;
    }
    setBusy(true);
    setError("");
    try {
      const safe = (item.name || "model").replace(/[^a-zA-Z0-9._-]+/g, "_");
      const dest =
        item.modelType === "LORA" ? `loras/${safe}.safetensors` : `checkpoints/${safe}.safetensors`;
      await api("/v1/civitai/install", {
        method: "POST",
        body: JSON.stringify({
          downloadUrl: item.downloadUrl,
          destPath: dest,
          name: item.name,
        }),
      });
      setSavedMsg(`Queued install of ${item.name} → ${dest} on the diffusion worker.`);
      if (item.examplePrompt) setPrompt(item.examplePrompt);
    } catch (e) {
      setError(e.message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="shell">
      <header className="hero">
        <h1 className="brand">Freya</h1>
        <p className="tagline">
          Describe what you want. Freya plans the model, enhances the prompt, and generates —
          cloud via OpenRouter, or your GPU worker with LoRAs.
        </p>
        <div className="compose">
          <textarea
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            placeholder="A quiet harbor at blue hour, warm windows, soft fog…"
          />
          <div className="row">
            <button className="primary" disabled={busy || !prompt.trim()} onClick={run}>
              {busy ? "Working…" : "Generate"}
            </button>
            <button className="ghost" disabled={busy || !prompt.trim()} onClick={enhanceOnly}>
              Enhance
            </button>
            {settings && (
              <span className="hint">
                <span className={`status-dot ${settings.openRouterConfigured ? "on" : ""}`} />
                OpenRouter
                <span
                  className={`status-dot ${settings.civitaiConfigured ? "on" : ""}`}
                  style={{ marginLeft: "0.75rem" }}
                />
                Civitai
                <span
                  className={`status-dot ${settings.diffusionWorkerUrl ? "on" : ""}`}
                  style={{ marginLeft: "0.75rem" }}
                />
                Worker
              </span>
            )}
          </div>
          {error && <p className="err">{error}</p>}
        </div>
      </header>

      {result && (
        <section className="result">
          <span className="badge">
            {result.backend}
            {result.model ? ` · ${result.model}` : ""}
          </span>
          <img src={result.url} alt={result.enhancedPrompt || prompt} />
        </section>
      )}

      <nav className="tabs">
        {["compose", "styles", "civitai", "gallery", "settings"].map((t) => (
          <button
            key={t}
            className={tab === t ? "active" : "ghost"}
            onClick={() => setTab(t)}
          >
            {t[0].toUpperCase() + t.slice(1)}
          </button>
        ))}
      </nav>

      {tab === "styles" && (
        <section className="panel grid">
          {styles.map((s) => (
            <article
              key={s.id}
              className="card"
              onClick={() => {
                setPrompt(s.examplePrompt);
                setTab("compose");
              }}
            >
              <h3>{s.displayName}</h3>
              <p>{s.examplePrompt.slice(0, 120)}…</p>
              <span className="badge">{s.backend}</span>
            </article>
          ))}
        </section>
      )}

      {tab === "civitai" && (
        <section className="panel">
          {!settings?.civitaiConfigured && (
            <p className="hint">
              Add your Civitai API key in Settings so Freya can search and install LoRAs onto your
              worker.
            </p>
          )}
          <div className="row" style={{ marginBottom: "1rem" }}>
            <input
              style={{ flex: 1, minWidth: "12rem" }}
              value={civitaiQ}
              onChange={(e) => setCivitaiQ(e.target.value)}
              placeholder="Search Civitai…"
            />
            <button className="primary" disabled={busy} onClick={searchCivitai}>
              Search
            </button>
          </div>
          {savedMsg && <p className="hint">{savedMsg}</p>}
          <div className="grid">
            {civitaiItems.map((item) => (
              <article key={item.id} className="card">
                <h3>{item.name}</h3>
                <p>{item.modelType}</p>
                <span className="badge">Civitai</span>
                <div className="row" style={{ marginTop: "0.75rem" }}>
                  <button
                    className="ghost"
                    onClick={() => {
                      if (item.examplePrompt) setPrompt(item.examplePrompt);
                      setTab("compose");
                    }}
                  >
                    Use prompt
                  </button>
                  <button className="primary" disabled={busy} onClick={() => installCivitai(item)}>
                    Install to worker
                  </button>
                </div>
              </article>
            ))}
          </div>
        </section>
      )}

      {tab === "gallery" && (
        <section className="panel grid">
          {gallery.map((g) => (
            <article
              key={g.id}
              className="card"
              onClick={async () => {
                const remix = await api(`/v1/gallery/${g.id}/remix`, { method: "POST" });
                setPrompt(remix.prompt || "");
                setTab("compose");
              }}
            >
              <img
                src={`/v1/gallery/${g.id}/image`}
                alt={g.prompt}
                style={{ width: "100%", borderRadius: 10, marginBottom: 8 }}
              />
              <p>{g.prompt.slice(0, 90)}…</p>
              <span className="badge">{g.backend}</span>
            </article>
          ))}
        </section>
      )}

      {tab === "settings" && (
        <section className="panel settings">
          <p className="hint">
            Keys stay on the Freya API (or in your Kubernetes Secrets). They are never shown back
            in full after save.
          </p>
          <label>
            OpenRouter API key
            <input
              type="password"
              autoComplete="off"
              placeholder={
                settings?.openRouterConfigured ? "Configured — paste to replace" : "sk-or-…"
              }
              value={openrouterKey}
              onChange={(e) => setOpenrouterKey(e.target.value)}
            />
          </label>
          <label>
            Civitai API key
            <input
              type="password"
              autoComplete="off"
              placeholder={
                settings?.civitaiConfigured
                  ? "Configured — paste to replace"
                  : "From civitai.com/user/account"
              }
              value={civitaiKey}
              onChange={(e) => setCivitaiKey(e.target.value)}
            />
            <span className="hint">
              Used to search Civitai and install LoRAs/checkpoints onto your diffusion worker for
              the UI.
            </span>
          </label>
          <label>
            Diffusion worker URL (in-cluster or remote GPU box)
            <input
              placeholder="http://gpu-box.lan:8091"
              value={workerUrl}
              onChange={(e) => setWorkerUrl(e.target.value)}
            />
          </label>
          <button className="primary" disabled={busy} onClick={saveSettings}>
            Save settings
          </button>
          {savedMsg && <p className="hint">{savedMsg}</p>}
        </section>
      )}
    </div>
  );
}
