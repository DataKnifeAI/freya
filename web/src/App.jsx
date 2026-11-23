import { useEffect, useRef, useState } from "react";
import { api, isMockMode } from "./api.js";
import HeroCanvas from "./HeroCanvas.jsx";
import {
  BEGINNER_CIVITAI_QUERY,
  BEGINNER_EXAMPLE,
  demoCivitaiQuery,
} from "./mockApi.js";

const AUTOMATION_MODES = ["plan", "auto", "yolo"];
const AUTOMATION_LABELS = { plan: "Plan", auto: "Auto", yolo: "YOLO" };
const AUTOMATION_STORAGE_KEY = "freya.automationMode";
const MODELS_STORAGE_KEY = "freya.models";
const SESSIONS_STORAGE_KEY = "freya.sessions";
const SIDEBAR_STORAGE_KEY = "freya.sidebarOpen";
const THEME_STORAGE_KEY = "freya.theme";
const TAB_STORAGE_KEY = "freya.tab";
/** Top-level Studio tabs (nav + restore must stay in sync). */
const STUDIO_TABS = [
  "gallery",
  "styles",
  "civitai",
  "worker",
  "metrics",
  "models",
  "settings",
  "help",
  "guidelines",
];

/** Empty Auto fallbacks → API uses Privacy/Alignment built-in matrix. */
const EMPTY_AUTO_FALLBACKS = {
  agent: "",
  agentZdr: "",
  image: "",
  imageZdr: "",
  video: "",
  videoZdr: "",
  router: "",
  costTier: "medium",
};

function normalizeAutoFallbacks(raw) {
  const r = raw && typeof raw === "object" ? raw : {};
  const pick = (k) => {
    const v = typeof r[k] === "string" ? r[k].trim() : "";
    return v.toLowerCase() === "auto" ? "" : v;
  };
  const cost = String(r.costTier || "medium").trim().toLowerCase();
  return {
    agent: pick("agent"),
    agentZdr: pick("agentZdr"),
    image: pick("image"),
    imageZdr: pick("imageZdr"),
    video: pick("video"),
    videoZdr: pick("videoZdr"),
    router: pick("router"),
    costTier: ["low", "high", "max"].includes(cost) ? cost : "medium",
  };
}

const COST_TIERS = [
  { id: "low", label: "Low" },
  { id: "medium", label: "Medium" },
  { id: "high", label: "High" },
  { id: "max", label: "Max" },
];

function costTierCurrent(value) {
  return COST_TIERS.find((t) => t.id === value) || COST_TIERS[1];
}

/** First-login / first-use acknowledgment of user liability guidelines. */
const GUIDELINES_ACK_STORAGE_KEY = "freya.guidelinesAck";
/** Persistent Civitai library (survives sessions). Distinct from session Pin. */
const FAVORITES_STORAGE_KEY = "freya.civitaiFavorites";

/** Third-party policy links (README + Settings + Guidelines tab stay in sync). */
const POLICY_LINKS = {
  openrouter: [
    { label: "Terms", href: "https://openrouter.ai/terms" },
    { label: "Privacy", href: "https://openrouter.ai/privacy" },
    {
      label: "Data collection",
      href: "https://openrouter.ai/docs/guides/privacy/data-collection",
    },
    { label: "ZDR", href: "https://openrouter.ai/docs/guides/features/zdr" },
  ],
  civitai: [
    { label: "Terms", href: "https://civitai.com/content/tos" },
    { label: "Privacy", href: "https://civitai.com/content/privacy" },
    { label: "Content moderation", href: "https://civitai.com/content/moderation" },
    { label: "Real-people rules", href: "https://civitai.com/content/rules/real-people" },
  ],
  huggingface: [
    { label: "Terms", href: "https://huggingface.co/terms-of-service" },
    { label: "Privacy", href: "https://huggingface.co/privacy" },
    { label: "Content policy", href: "https://huggingface.co/content-policy" },
  ],
};

function readGuidelinesAck() {
  try {
    return localStorage.getItem(GUIDELINES_ACK_STORAGE_KEY) === "1";
  } catch {
    return false;
  }
}

function persistGuidelinesAck() {
  try {
    localStorage.setItem(GUIDELINES_ACK_STORAGE_KEY, "1");
  } catch {
    /* private mode / quota */
  }
}

function PolicyLinkRow({ links }) {
  return links.map((l, i) => (
    <span key={l.href}>
      {i > 0 ? " · " : null}
      <a href={l.href} target="_blank" rel="noreferrer">
        {l.label}
      </a>
    </span>
  ));
}

/** Compact “?” control — opens Help tab at a section id (`help-zdr`, …). */
function HelpQ({ topic, onOpen, label }) {
  const title = label ? `Help: ${label}` : "Help";
  return (
    <button
      type="button"
      className="help-q"
      title={title}
      aria-label={title}
      onClick={(e) => {
        e.preventDefault();
        e.stopPropagation();
        onOpen?.(topic);
      }}
    >
      ?
    </button>
  );
}

function HelpSection({ id, title, children, active }) {
  return (
    <article id={id} className={`help-section${active ? " is-active" : ""}`}>
      <h3 className="help-section-title">{title}</h3>
      <div className="help-section-body">{children}</div>
    </article>
  );
}

function GuidelinesBody({ compact = false }) {
  return (
    <div className={compact ? "guidelines-body is-compact" : "guidelines-body"}>
      <p>
        <strong>Freya is a tool.</strong> It helps you plan prompts and call image/chat APIs you
        configure. It does not decide what is legal where you live, and it does not police your
        prompts or outputs beyond what third-party providers already enforce.
      </p>
      <p>
        <strong>You are responsible.</strong> Generated or downloaded content may be illegal or
        restricted under your country or state law. You alone are liable for how you use Freya,
        what you generate, and what you publish or share — not Freya, DataKnifeAI, or the
        operators of this instance.
      </p>
      <p>
        <strong>Bring your own keys and hardware.</strong> You supply OpenRouter / Civitai / HF
        credentials and any local GPU worker. Those accounts and machines are yours; Freya only
        relays requests you initiate.
      </p>
      <p>
        <strong>Follow third-party policies.</strong> Using Freya means you agree to comply with
        each provider’s terms, privacy, and content rules for any keys you configure and any
        prompts or assets you send or download:
      </p>
      <ul className="guidelines-policy-list">
        <li>
          <strong>OpenRouter</strong> — <PolicyLinkRow links={POLICY_LINKS.openrouter} />
        </li>
        <li>
          <strong>Civitai</strong> — <PolicyLinkRow links={POLICY_LINKS.civitai} />
        </li>
        <li>
          <strong>Hugging Face</strong> — <PolicyLinkRow links={POLICY_LINKS.huggingface} />
        </li>
      </ul>
      {!compact && (
        <p className="hint">
          Acknowledgment is stored in this browser as{" "}
          <code>{GUIDELINES_ACK_STORAGE_KEY}</code>. Clear site data to see the prompt again.
        </p>
      )}
    </div>
  );
}
const CIVITAI_SUBVIEW_STORAGE_KEY = "freya.civitaiSubview";
/** Left cluster then Downloads (rightmost in subnav). */
const CIVITAI_SUBVIEWS = ["search", "favorites", "installed", "downloads"];
/** Debounce for Civitai search-as-you-type (ms). */
const CIVITAI_SEARCH_DEBOUNCE_MS = 350;
const CIVITAI_SEARCH_TYPES = "LORA,LoCon,DoRA,Checkpoint,TextualInversion,VAE";
/** Local freya-worker / freya-local-worker defaults (see .env.example, docs/LOCAL.md). */
const DEFAULT_WORKER_HOST = "localhost";
const DEFAULT_WORKER_PORT = "8091";

/** Status suffix for active base — reflects Diffusers engine, not stub-era copy. */
function activeBaseEngineHint(modelLoaded, workerStats) {
  if (!modelLoaded?.model) return "";
  const engineName =
    modelLoaded.engine ||
    workerStats?.engine?.engine ||
    workerStats?.worker?.engine?.engine ||
    "";
  const enginePayload = workerStats?.engine || workerStats?.worker?.engine || null;
  const engineOk = enginePayload?.ok === true;
  if (modelLoaded.loadedInVram === true) {
    return engineName ? ` — loaded in VRAM (${engineName})` : " — loaded in VRAM";
  }
  if (engineName === "diffusers-unreachable") {
    return " — selected; Diffusers engine unavailable";
  }
  if (engineName === "diffusers" || (engineOk && engineName)) {
    return " — selected; Diffusers loads on demand";
  }
  if (!engineName || engineName === "stub") {
    return " — selected on disk; no Diffusers engine configured";
  }
  return ` — selected (${engineName})`;
}

/** Label container vs host models path when both are known. */
function modelsDirHint(workerStats) {
  const dir = workerStats?.modelsDir;
  if (!dir) return null;
  const host = workerStats?.modelsDirHost || workerStats?.worker?.modelsDirHost || null;
  const containerish =
    dir === "/models" || dir === "/data/models" || dir.startsWith("/models/");
  if (host) {
    return `Models dir: ${host} (container: ${dir})`;
  }
  if (containerish) {
    return `Models dir (container path): ${dir}`;
  }
  return `Models dir: ${dir}`;
}

function parseWorkerEndpoint(url) {
  // No saved URL → show local defaults in the fields (hasWorker still uses persisted settings).
  const empty = {
    host: DEFAULT_WORKER_HOST,
    port: DEFAULT_WORKER_PORT,
    advancedUrl: "",
    useAdvanced: false,
  };
  if (!url || !String(url).trim()) return empty;
  try {
    const u = new URL(String(url).trim());
    const path = (u.pathname || "/").replace(/\/$/, "") || "";
    const simpleHttp =
      u.protocol === "http:" && !path && !u.search && !u.hash && Boolean(u.hostname);
    if (simpleHttp) {
      return {
        host: u.hostname,
        port: u.port || DEFAULT_WORKER_PORT,
        advancedUrl: "",
        useAdvanced: false,
      };
    }
    return {
      host: "",
      port: "",
      advancedUrl: String(url).trim(),
      useAdvanced: true,
    };
  } catch {
    return {
      host: "",
      port: "",
      advancedUrl: String(url).trim(),
      useAdvanced: true,
    };
  }
}

function composeWorkerUrl(host, port) {
  const h = String(host || "").trim();
  if (!h) return "";
  const p = String(port || DEFAULT_WORKER_PORT).trim() || DEFAULT_WORKER_PORT;
  return `http://${h}:${p}`;
}

/** Studio origin for setup script URLs (same host that serves /scripts/…). */
function studioOrigin() {
  try {
    if (typeof window !== "undefined" && window.location?.origin) {
      return window.location.origin;
    }
  } catch {
    /* ignore */
  }
  return "";
}

function shellSingleQuote(s) {
  return `'${String(s ?? "").replace(/'/g, `'\\''`)}'`;
}

/** Linux one-liner with machine token substituted (Worker tab Setup). */
function buildLinuxWorkerSetupCmd({ origin, token, apiUrl }) {
  const base = (origin || "").replace(/\/$/, "") || "<studio-origin>";
  const tok = token?.trim() || "<machine-token>";
  const api = (apiUrl || base).replace(/\/$/, "");
  return `curl -fsSL ${base}/scripts/setup-local-worker.sh | bash -s -- --token ${shellSingleQuote(tok)} --api-url ${shellSingleQuote(api)}`;
}

/** Windows PowerShell one-liner with machine token substituted. */
function buildWindowsWorkerSetupCmd({ origin, token, apiUrl }) {
  const base = (origin || "").replace(/\/$/, "") || "<studio-origin>";
  const tok = (token?.trim() || "<machine-token>").replace(/'/g, "''");
  const api = ((apiUrl || base).replace(/\/$/, "") || base).replace(/'/g, "''");
  // PowerShell subexpression $(irm …) — escape $ so JS template literals leave it alone.
  return `iex "& { \$(irm '${base}/scripts/setup-local-worker.ps1') } -Token '${tok}' -ApiUrl '${api}'"`;
}

function readFavorites() {
  try {
    const raw = localStorage.getItem(FAVORITES_STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter((x) => x && (x.id != null || x.name)) : [];
  } catch {
    return [];
  }
}

function writeFavorites(list) {
  try {
    localStorage.setItem(FAVORITES_STORAGE_KEY, JSON.stringify(list.slice(0, 200)));
  } catch {
    /* ignore quota */
  }
}

/** Human-readable byte size for storage meters (Worker tab). */
function formatBytes(n) {
  const v = Number(n);
  if (!Number.isFinite(v) || v < 0) return "—";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let x = v;
  let i = 0;
  while (x >= 1024 && i < units.length - 1) {
    x /= 1024;
    i += 1;
  }
  const digits = i === 0 ? 0 : x >= 10 ? 1 : 2;
  return `${x.toFixed(digits)} ${units[i]}`;
}

/** Compact used/total progress bar (gallery PVC + worker models). */
function StorageMeter({ label, disk, fallbackHint }) {
  const used = disk?.usedBytes;
  const total = disk?.totalBytes;
  const free = disk?.freeBytes;
  const pct =
    typeof disk?.usedPercent === "number"
      ? Math.min(100, Math.max(0, Math.round(disk.usedPercent)))
      : typeof used === "number" && typeof total === "number" && total > 0
        ? Math.min(100, Math.round((used / total) * 100))
        : null;
  const meta =
    pct != null && typeof used === "number" && typeof total === "number"
      ? `${formatBytes(used)} / ${formatBytes(total)} (${pct}%)${
          typeof free === "number" ? ` · ${formatBytes(free)} free` : ""
        }`
      : fallbackHint || "n/a";
  return (
    <div className="worker-vram worker-disk" title={disk?.path || undefined}>
      <div className="worker-vram-meta">
        <span>{label}</span>
        <span>{meta}</span>
      </div>
      <div
        className="worker-vram-bar"
        role="progressbar"
        aria-label={label}
        aria-valuenow={pct ?? 0}
        aria-valuemin={0}
        aria-valuemax={100}
      >
        <div className="worker-vram-fill" style={{ width: `${pct ?? 0}%` }} />
      </div>
    </div>
  );
}

function formatStat(n) {
  if (n == null || Number.isNaN(Number(n))) return null;
  const v = Number(n);
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
  if (v >= 1_000) return `${(v / 1_000).toFixed(1)}K`;
  return String(v);
}

/** Format OpenRouter USD credit amounts for the Metrics tab. */
function fmtUsd(n) {
  if (n == null || Number.isNaN(Number(n))) return "—";
  const v = Number(n);
  const abs = Math.abs(v);
  const digits = abs >= 100 ? 2 : abs >= 1 ? 3 : 4;
  return `$${v.toFixed(digits)}`;
}

function readStoredTheme() {
  try {
    const v = localStorage.getItem(THEME_STORAGE_KEY);
    if (v === "light" || v === "dark") return v;
  } catch {
    /* ignore */
  }
  return null;
}

function systemTheme() {
  try {
    return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
  } catch {
    return "dark";
  }
}

/** Saved `freya.theme` wins; otherwise follow `prefers-color-scheme`. */
function resolveInitialTheme() {
  const fromDom = document.documentElement.dataset.theme;
  if (fromDom === "light" || fromDom === "dark") return fromDom;
  return readStoredTheme() || systemTheme();
}

const MODE_HINTS = {
  plan: "Plan only — steps, Civitai query, LoRAs, batch prompts. No generate or install.",
  auto: "Run & ask when unsure — pauses on ambiguous LoRAs. Installs only if a worker is set; otherwise generates via OpenRouter.",
  yolo: "Full auto — search, install only when a worker is set, otherwise generate via OpenRouter. No questions.",
};

const AGENT_MODELS = [
  { id: "openrouter/auto", label: "Auto", short: "Auto" },
  { id: "anthropic/claude-sonnet-4", label: "Claude Sonnet 4", short: "Sonnet 4" },
  { id: "openai/gpt-4.1-mini", label: "GPT-4.1 Mini", short: "GPT-4.1 Mini" },
  { id: "google/gemini-2.5-flash", label: "Gemini 2.5 Flash", short: "Gemini Flash" },
];

const IMAGE_MODELS = [
  { id: "auto", label: "Auto", short: "Auto" },
  { id: "black-forest-labs/flux.2-klein-4b", label: "FLUX.2 Klein 4B", short: "FLUX Klein" },
  { id: "black-forest-labs/flux.1-schnell", label: "FLUX.1 Schnell", short: "FLUX Schnell" },
  { id: "stabilityai/sdxl-turbo", label: "SDXL Turbo", short: "SDXL Turbo" },
];

const emptySession = () => ({
  goal: "",
  pinnedLoras: [],
  turns: [],
  sessionNote: "",
});

function makeSessionId() {
  return `s_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 7)}`;
}

function sessionTitleFrom(prompt, session) {
  const raw = (session?.goal || prompt || "").trim().replace(/\s+/g, " ");
  if (!raw) return "Untitled session";
  return raw.length > 52 ? `${raw.slice(0, 52)}…` : raw;
}

function sessionImageCount(entry) {
  const fromTurns = (entry?.session?.turns || []).reduce(
    (n, t) => n + (t.artifactIds?.length || 0),
    0,
  );
  if (fromTurns) return fromTurns;
  const results = entry?.results;
  if (Array.isArray(results) && results.length) return results.length;
  return entry?.result ? 1 : 0;
}

function relativeTime(iso) {
  if (!iso) return "";
  const t = new Date(iso).getTime();
  if (Number.isNaN(t)) return "";
  const sec = Math.max(0, Math.floor((Date.now() - t) / 1000));
  if (sec < 45) return "just now";
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const d = Math.floor(hr / 24);
  return `${d}d ago`;
}

function SessionRow({
  title,
  updatedAt,
  createdAt,
  imageCount,
  active,
  archived,
  confirming,
  onOpen,
  onArchive,
  onUnarchive,
  onAskDelete,
  onConfirmDelete,
  onCancelDelete,
}) {
  return (
    <li className={`session-row${active ? " is-active" : ""}`}>
      <button
        type="button"
        role="option"
        aria-selected={active}
        className={`session-item${active ? " active" : ""}`}
        onClick={() => {
          onCancelDelete();
          onOpen();
        }}
      >
        <span className="session-item-title">{title}</span>
        <span className="session-item-meta">
          <time dateTime={updatedAt || createdAt}>{relativeTime(updatedAt || createdAt)}</time>
          {imageCount > 0 && (
            <span>
              {imageCount} img{imageCount === 1 ? "" : "s"}
            </span>
          )}
        </span>
      </button>
      <div className={`session-row-actions${confirming ? " is-confirm" : ""}`}>
        {confirming ? (
          <>
            <button
              type="button"
              className="ghost danger-ghost session-confirm"
              title="Delete this session"
              aria-label="Confirm delete"
              onClick={onConfirmDelete}
            >
              Delete?
            </button>
            <button
              type="button"
              className="ghost session-confirm"
              title="Cancel"
              aria-label="Cancel delete"
              onClick={onCancelDelete}
            >
              No
            </button>
          </>
        ) : (
          <>
            {archived ? (
              <button
                type="button"
                className="ghost icon-btn"
                title="Unarchive"
                aria-label="Unarchive session"
                onClick={onUnarchive}
              >
                <IconUnarchive />
              </button>
            ) : (
              <button
                type="button"
                className="ghost icon-btn"
                title="Archive"
                aria-label="Archive session"
                onClick={onArchive}
              >
                <IconArchive />
              </button>
            )}
            <button
              type="button"
              className="ghost danger-ghost icon-btn"
              title="Delete"
              aria-label="Delete session"
              onClick={onAskDelete}
            >
              <IconTrash />
            </button>
          </>
        )}
      </div>
    </li>
  );
}

function blankSessionEntry(id = makeSessionId()) {
  const now = new Date().toISOString();
  return {
    id,
    title: "Untitled session",
    createdAt: now,
    updatedAt: now,
    prompt: "",
    session: emptySession(),
    ideas: [],
    suggestedLoras: [],
    suggestNote: "",
    civitaiQuery: "",
    result: null,
    results: [],
    modelBrief: "",
    pendingPlan: null,
    pendingQuestion: null,
    sessionLog: [],
  };
}

function readStoredSidebarOpen() {
  try {
    const v = localStorage.getItem(SIDEBAR_STORAGE_KEY);
    if (v === "0" || v === "false") return false;
    if (v === "1" || v === "true") return true;
  } catch {
    /* ignore */
  }
  return true;
}

function readStoredSessions() {
  try {
    const raw = localStorage.getItem(SESSIONS_STORAGE_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw);
    if (!parsed || !Array.isArray(parsed.sessions) || !parsed.sessions.length) return null;
    return {
      activeId: parsed.activeId || parsed.sessions[0].id,
      sessions: parsed.sessions,
    };
  } catch {
    /* ignore */
  }
  return null;
}

function persistSessionsStore(activeId, sessions) {
  try {
    localStorage.setItem(
      SESSIONS_STORAGE_KEY,
      JSON.stringify({ activeId, sessions }),
    );
  } catch {
    /* ignore */
  }
}

function loraKey(l) {
  return String(l.id ?? l.name);
}

function readStoredMode() {
  try {
    const v = localStorage.getItem(AUTOMATION_STORAGE_KEY);
    if (v === "review") return "plan";
    if (v === "approve") return "auto";
    if (AUTOMATION_MODES.includes(v)) return v;
  } catch {
    /* ignore */
  }
  return "plan";
}

function readStoredModels() {
  try {
    const raw = localStorage.getItem(MODELS_STORAGE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      return {
        agentModel: AGENT_MODELS.some((m) => m.id === parsed.agentModel)
          ? parsed.agentModel
          : "openrouter/auto",
        imageModel: IMAGE_MODELS.some((m) => m.id === parsed.imageModel)
          ? parsed.imageModel
          : "auto",
      };
    }
  } catch {
    /* ignore */
  }
  return { agentModel: "openrouter/auto", imageModel: "auto" };
}

function artifactExt(item, src) {
  const named = String(item?.filename || "");
  const fromName = named.match(/\.([a-z0-9]{2,5})$/i);
  if (fromName) return fromName[1].toLowerCase();
  const m = String(artifactMime(item) || "").toLowerCase();
  if (m.includes("webp")) return "webp";
  if (m.includes("gif")) return "gif";
  if (m.includes("jpeg") || m.includes("jpg")) return "jpg";
  if (m.includes("webm")) return "webm";
  if (m.includes("mp4") || m.includes("video")) return "mp4";
  if (m.includes("png")) return "png";
  const s = String(src || "").toLowerCase();
  if (s.includes(".webp")) return "webp";
  if (s.includes(".gif")) return "gif";
  if (s.includes(".mp4")) return "mp4";
  if (s.includes(".webm")) return "webm";
  if (isVideoArtifact(item)) return "mp4";
  if (isGifArtifact(item)) return "gif";
  return "png";
}

function artifactFilename(item, src) {
  const existing = String(item?.filename || "");
  if (existing && !existing.includes("/") && !existing.includes("\\")) return existing;
  const id =
    String(item?.id || item?.artifactId || "image")
      .replace(/[^a-z0-9-]/gi, "")
      .slice(0, 8) || "image";
  const slug = String(item?.prompt || "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 32);
  return `${slug ? `freya-${slug}-${id}` : `freya-${id}`}.${artifactExt(item, src)}`;
}

/** Save the gallery/artifact bytes. Does not open a tab. */
async function downloadArtifact(src, item) {
  const filename = artifactFilename(item, src);
  const res = await fetch(src);
  if (!res.ok) throw new Error(`Download failed (${res.status})`);
  const blob = await res.blob();
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1500);
}

function installableLoras(loras = []) {
  return (loras || []).filter((l) => l.downloadUrl);
}

function modelLabel(list, id) {
  return list.find((m) => m.id === id)?.label || id;
}

function modelShortLabel(list, id) {
  const m = list.find((x) => x.id === id);
  return m?.short || m?.label || id;
}

/* Small inline glyphs (stroke = currentColor) — icons first, words in tooltips. */
const Svg = ({ children, filled = false }) => (
  <svg
    viewBox="0 0 24 24"
    width="14"
    height="14"
    fill={filled ? "currentColor" : "none"}
    stroke="currentColor"
    strokeWidth="1.8"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
    focusable="false"
  >
    {children}
  </svg>
);
const IconPlay = () => (
  <Svg filled>
    <path d="M8 5.5v13l11-6.5z" stroke="none" />
  </Svg>
);
const IconSpinner = () => (
  <span className="icon-spin">
    <Svg>
      <path d="M12 3a9 9 0 1 0 9 9" />
    </Svg>
  </span>
);
const IconBulb = () => (
  <Svg>
    <path d="M9 18h6M10 21h4" />
    <path d="M12 3a6 6 0 0 0-3.6 10.8c.6.5 1 1.1 1.1 2.2h5c.1-1.1.5-1.7 1.1-2.2A6 6 0 0 0 12 3z" />
  </Svg>
);
const IconLayers = () => (
  <Svg>
    <path d="M12 3 3 8l9 5 9-5-9-5z" />
    <path d="M3 12.5l9 5 9-5" />
  </Svg>
);
const IconSliders = () => (
  <Svg>
    <path d="M5 7h9M18 7h1M5 17h3M12 17h7" />
    <circle cx="16" cy="7" r="2" />
    <circle cx="10" cy="17" r="2" />
  </Svg>
);
const IconAgent = () => (
  <Svg>
    <rect x="4" y="9" width="16" height="10" rx="2" />
    <path d="M12 9V5M12 5h.01M8.5 14h.01M15.5 14h.01M9.5 17h5" />
  </Svg>
);
const IconImage = () => (
  <Svg>
    <rect x="3" y="5" width="18" height="14" rx="2" />
    <circle cx="8.5" cy="10" r="1.4" />
    <path d="M21 15.5 16 11l-8.5 8" />
  </Svg>
);
const IconChevron = () => (
  <Svg>
    <path d="m6 9 6 6 6-6" />
  </Svg>
);
const IconTrash = () => (
  <Svg>
    <path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6" />
    <path d="M10 11v6M14 11v6" />
  </Svg>
);
const IconArchive = () => (
  <Svg>
    <path d="M4 7h16v4H4z" />
    <path d="M6 11v8h12v-8" />
    <path d="M10 15h4" />
  </Svg>
);
const IconUnarchive = () => (
  <Svg>
    <path d="M4 7h16v4H4z" />
    <path d="M6 11v8h12v-8" />
    <path d="M12 18V13M9.5 15.5 12 13l2.5 2.5" />
  </Svg>
);
function MediaDownload({ src, item, className, onError }) {
  if (!src) return null;
  return (
    <button
      type="button"
      className={className || "ghost icon-btn"}
      title="Download"
      aria-label="Download file"
      onClick={(e) => {
        e.stopPropagation();
        e.preventDefault();
        downloadArtifact(src, item).catch((err) =>
          onError?.(err?.message || "Download failed"),
        );
      }}
    >
      <IconDownload />
    </button>
  );
}
const IconSessions = () => (
  <Svg>
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <path d="M9 4v16" />
  </Svg>
);
const IconSun = () => (
  <Svg>
    <circle cx="12" cy="12" r="4" />
    <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
  </Svg>
);
const IconMoon = () => (
  <Svg>
    <path d="M20 13.5A8.5 8.5 0 1 1 10.5 4a6.8 6.8 0 0 0 9.5 9.5z" />
  </Svg>
);

function CompactMenu({ label, valueLabel, options, value, onChange, open, onOpen, icon, title }) {
  const ref = useRef(null);

/** Installed section order / labels by kind or modelType. */
const INSTALLED_SECTION_ORDER = [
  { id: "loras", title: "LoRAs", match: (t) => ["lora", "locon", "dora", "lycoris"].includes(t) },
  { id: "checkpoints", title: "Checkpoints", match: (t) => t === "checkpoint" },
  { id: "models", title: "Models", match: (t) => t === "model" },
  {
    id: "embeddings",
    title: "Embeddings",
    match: (t) => t === "textualinversion" || t === "embedding",
  },
  { id: "vae", title: "VAEs", match: (t) => t === "vae" },
];

function installedSectionId(asset) {
  const raw = String(asset?.modelType || asset?.kind || "").toLowerCase();
  // Prefer path prefix when kind/type is missing or generic.
  const path = String(asset?.path || "");
  if (path.startsWith("loras/")) return "loras";
  if (path.startsWith("checkpoints/")) return "checkpoints";
  if (path.startsWith("embeddings/")) return "embeddings";
  if (path.startsWith("vae/")) return "vae";
  for (const sec of INSTALLED_SECTION_ORDER) {
    if (sec.id === "models") continue; // only via explicit kind
    if (sec.match(raw)) return sec.id;
  }
  if (raw === "model") return "models";
  return "models";
}

function groupInstalledAssets(assets) {
  const buckets = new Map();
  for (const a of assets || []) {
    const id = installedSectionId(a);
    if (!buckets.has(id)) buckets.set(id, []);
    buckets.get(id).push(a);
  }
  const order = ["loras", "checkpoints", "models", "embeddings", "vae"];
  const titles = {
    loras: "LoRAs",
    checkpoints: "Checkpoints",
    models: "Models",
    embeddings: "Embeddings",
    vae: "VAEs",
  };
  const out = [];
  for (const id of order) {
    const items = buckets.get(id);
    if (items?.length) out.push({ id, title: titles[id], items });
  }
  for (const [id, items] of buckets) {
    if (!order.includes(id) && items.length) {
      out.push({ id, title: id.charAt(0).toUpperCase() + id.slice(1), items });
    }
  }
  return out;
}

const CIVITAI_THUMB_CACHE_KEY = "freya.civitaiThumbCache";

function readThumbCache() {
  try {
    const raw = localStorage.getItem(CIVITAI_THUMB_CACHE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch {
    return {};
  }
}

function writeThumbCache(map) {
  try {
    localStorage.setItem(CIVITAI_THUMB_CACHE_KEY, JSON.stringify(map));
  } catch {
    /* ignore */
  }
}

function thumbCacheKey(item) {
  const mid = item?.modelId ?? item?.civitaiModelId;
  if (mid != null && mid !== "") return `m:${mid}`;
  if (item?.versionId != null && item.versionId !== "") return `v:${item.versionId}`;
  return null;
}

function rememberCivitaiThumb(item, url) {
  if (!url) return;
  const key = thumbCacheKey(item);
  if (!key) return;
  const map = readThumbCache();
  if (map[key] === url) return;
  map[key] = url;
  const keys = Object.keys(map);
  if (keys.length > 400) {
    for (const k of keys.slice(0, keys.length - 300)) delete map[k];
  }
  writeThumbCache(map);
}

function cachedCivitaiThumb(item) {
  const key = thumbCacheKey(item);
  if (!key) return null;
  return readThumbCache()[key] || null;
}

/** Prefer real image URLs — meta, imageUrls, localStorage cache, then favorites. */
function pickCivitaiThumb(item, favoritesList = []) {
  if (!item) return null;
  const direct = item.thumbnailUrl || item.imageUrls?.[0] || null;
  if (direct) return direct;
  const cached = cachedCivitaiThumb(item);
  if (cached) return cached;
  const mid = resolveCivitaiModelId(item);
  const vid = item.versionId;
  const fromFav = (favoritesList || []).find(
    (f) =>
      (mid != null && resolveCivitaiModelId(f) === mid) ||
      (vid != null && String(f.versionId) === String(vid)),
  );
  return fromFav?.thumbnailUrl || fromFav?.imageUrls?.[0] || null;
}

/** Keep in sync with order_lookup_assets in crates/freya-api/src/routes.rs. */
function assetLabelKey(raw) {
  const file = String(raw || "").split(/[/\\]/).pop() || "";
  const stem = file.replace(/\.(safetensors|ckpt|pt|gguf|bin)$/i, "");
  return stem.toLowerCase().replace(/[^a-z0-9]+/g, "");
}

function queryHitsLabel(query, label) {
  const q = assetLabelKey(query);
  const n = assetLabelKey(label);
  if (q.length < 3 || !n) return false;
  if (q === n) return true;
  return (q.length >= 4 && n.includes(q)) || (n.length >= 4 && q.includes(n));
}

function cardVersionId(item) {
  const n = Number(item?.versionId);
  return Number.isFinite(n) && n > 0 ? n : null;
}

/** Same installed file: equal version ids, or equal name/stem when a version is missing.
 * Different version ids stay distinct so the other version remains a new hit.
 */
function lookupSameAsset(local, search) {
  if (!local || !search) return false;
  const lv = cardVersionId(local);
  const sv = cardVersionId(search);
  if (lv && sv) return lv === sv;
  const localKeys = [
    assetLabelKey(local.name),
    assetLabelKey(local.path),
    assetLabelKey(local.fileName),
  ].filter(Boolean);
  const searchKeys = [assetLabelKey(search.name), assetLabelKey(search.fileName)].filter(Boolean);
  return localKeys.some((lk) => searchKeys.includes(lk));
}

/** Query text that is a Civitai version id (the whole query, or a 4+ digit token). */
function queryHitsVersion(query, item) {
  const vid = cardVersionId(item);
  if (!vid || !query) return false;
  const q = String(query).trim();
  if (q === String(vid)) return true;
  return q.split(/[^0-9]+/).some((tok) => tok.length >= 4 && Number(tok) === vid);
}

/** Search hit from this chat lookup. A worker-library row is not a search hit unless stamped. */
function isLookupSearchCard(card) {
  if (!card) return false;
  if (card.fromChatSearch) return true;
  if (card.path) return false;
  return (
    resolveCivitaiModelId(card) != null ||
    cardVersionId(card) != null ||
    Boolean(card.downloadUrl || card.civitaiUrl || card.thumbnailUrl || card.imageUrls?.length)
  );
}

/** Installed file matches the chat lookup by name, filename, version id, or a search hit. */
function installedMatchesLookup(local, query, searchHits) {
  if (!local) return false;
  const q = String(query || "").trim();
  if (q) {
    if (queryHitsLabel(q, local.name)) return true;
    if (queryHitsLabel(q, local.path)) return true;
    if (queryHitsLabel(q, local.fileName)) return true;
    if (queryHitsVersion(q, local)) return true;
  }
  return (searchHits || []).some(
    (c) =>
      lookupSameAsset(local, c) ||
      Boolean(local.path && c?.path && local.path === c.path),
  );
}

function findInstalledMatch(item, installed) {
  if (!item || !installed?.length) return null;
  if (item.path) {
    const byPath = installed.find((a) => a?.path && a.path === item.path);
    if (byPath) return byPath;
  }
  return installed.find((a) => lookupSameAsset(a, item)) || null;
}

function mergeInstalledCard(card, live) {
  const searchModel = !card?.path ? resolveCivitaiModelId(card) : null;
  const modelId = live?.modelId ?? card?.modelId ?? searchModel ?? null;
  return {
    ...(card || {}),
    ...(live || {}),
    modelId: modelId || null,
    versionId: live?.versionId ?? card?.versionId ?? null,
    thumbnailUrl: live?.thumbnailUrl || card?.thumbnailUrl || null,
    imageUrls: live?.imageUrls?.length ? live.imageUrls : card?.imageUrls,
    path: live?.path || card?.path || null,
    name: live?.name || card?.name,
    modelType: live?.modelType || live?.kind || card?.modelType,
    baseModel: live?.baseModel || card?.baseModel,
    triggerWord: live?.triggerWord || card?.triggerWord,
    civitaiUrl: live?.civitaiUrl || card?.civitaiUrl,
    downloadUrl: live?.downloadUrl || card?.downloadUrl,
    fromChatSearch: Boolean(card?.fromChatSearch || live?.fromChatSearch),
    installed: true,
  };
}

function lookupCardKey(item) {
  if (item?.path) return `p:${item.path}`;
  const ver = cardVersionId(item);
  if (ver) return `v:${ver}`;
  return `n:${assetLabelKey(item?.name)}:${item?.id ?? ""}`;
}

/**
 * Installed rows: only worker files that match this chat lookup
 * (name, filename, or version id — or the same asset as a search hit).
 * Other search hits stay in `fresh`. A full library dump is not listed.
 */
function presentLookup(cards, installed, query) {
  const existing = [];
  const fresh = [];
  const seen = new Set();
  const pushExisting = (item) => {
    const key = lookupCardKey(item);
    if (!key || seen.has(key)) return;
    seen.add(key);
    existing.push(item);
  };
  const list = Array.isArray(cards) ? cards.filter(Boolean) : [];
  const searchHits = list.filter(isLookupSearchCard);
  const q = String(query || "").trim();
  const candidates = [];
  const seenPath = new Set();
  const pushCandidate = (asset) => {
    if (!asset) return;
    if (asset.path) {
      if (seenPath.has(asset.path)) return;
      seenPath.add(asset.path);
    }
    candidates.push(asset);
  };
  for (const local of installed || []) pushCandidate(local);
  for (const card of list) {
    if (card.path) pushCandidate(card);
  }
  for (const local of candidates) {
    const searchedFile = Boolean(local.fromChatSearch && local.path);
    if (!searchedFile && !installedMatchesLookup(local, q, searchHits)) continue;
    if (
      existing.some(
        (e) => lookupSameAsset(e, local) || (local.path && e.path === local.path),
      )
    ) {
      continue;
    }
    const hit = searchHits.find((c) => c !== local && lookupSameAsset(local, c));
    pushExisting(mergeInstalledCard(hit || local, local));
  }
  for (const card of searchHits) {
    if (card.path && existing.some((e) => e.path === card.path)) continue;
    if (existing.some((e) => lookupSameAsset(e, card))) continue;
    const key = lookupCardKey(card);
    if (!key || seen.has(key)) continue;
    seen.add(key);
    fresh.push({ ...card, installed: false });
  }
  return { existing, fresh };
}

/** Prefer the search payload (already installed-first). Otherwise list installed/local. */
function cardsFromChatPayload(data) {
  const actions = Array.isArray(data?.actions) ? data.actions : [];
  const searches = actions.filter(
    (a) => a?.type === "search_civitai" && a.status === "executed" && Array.isArray(a.result?.loras),
  );
  if (searches.length) {
    return searches.flatMap((a) =>
      a.result.loras.filter(Boolean).map((c) => ({ ...c, fromChatSearch: true })),
    );
  }
  const listed = [];
  for (const a of actions) {
    if (a?.status !== "executed" || !a.result) continue;
    if (a.type === "list_loras" && Array.isArray(a.result.loras)) listed.push(...a.result.loras);
    if (a.type === "list_checkpoints" && Array.isArray(a.result.checkpoints)) {
      listed.push(...a.result.checkpoints);
    }
    if (a.type === "list_installed") {
      for (const key of ["loras", "checkpoints", "embeddings", "vaes"]) {
        if (Array.isArray(a.result[key])) listed.push(...a.result[key]);
      }
    }
  }
  if (listed.length) return listed;
  return Array.isArray(data?.loras) ? data.loras : [];
}

/** Dismiss helpers shared by compact + grouped chrome menus. */
function useChromeMenuDismiss(open, onOpen, ref) {
  useEffect(() => {
    if (!open) return;
    const onDoc = (e) => {
      if (ref.current && !ref.current.contains(e.target)) onOpen(false);
    };
    const onKey = (e) => {
      if (e.key === "Escape") onOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
    };
  }, [open, onOpen, ref]);
}

/**
 * $ icon → popover list (same chrome as the model menus).
 * One pick sets the cost tier and closes.
 */
function CostTierMenu({ value, onChange, open, onOpen }) {
  const ref = useRef(null);
  useChromeMenuDismiss(open, onOpen, ref);
  const current = costTierCurrent(value);

  return (
    <div className="chrome-menu chrome-menu-align-end composer-cost-menu" ref={ref}>
      <button
        type="button"
        className={`ghost icon-btn${open ? " open" : ""}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={`Cost: ${current.label}`}
        title={`Cost: ${current.label}`}
        onClick={() => onOpen(!open)}
      >
        <span className="cost-tier-mark" aria-hidden="true">$</span>
        <span className="sr-only">Cost: {current.label}</span>
      </button>
      {open && (
        <div className="chrome-popover" role="dialog" aria-label={`Cost: ${current.label}`}>
          <div className="cost-tier-menu-current">
            <span>Cost</span>
            <span>{current.label}</span>
          </div>
          <ul className="chrome-popover-list" role="listbox" aria-label="Cost tier">
            {COST_TIERS.map((t) => (
              <li key={t.id}>
                <button
                  type="button"
                  role="option"
                  aria-selected={t.id === current.id}
                  className={t.id === current.id ? "active" : ""}
                  onClick={() => {
                    onChange(t.id);
                    onOpen(false);
                  }}
                >
                  <span className="chrome-menu-opt-label">{t.label}</span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

function CompactMenu({
  label,
  valueLabel,
  options,
  value,
  onChange,
  open,
  onOpen,
  icon,
  title,
  menuNote,
}) {
  const ref = useRef(null);
  useChromeMenuDismiss(open, onOpen, ref);

  return (
    <div className="chrome-menu" ref={ref}>
      <button
        type="button"
        className={`chrome-chip${open ? " open" : ""}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={`${label}: ${valueLabel}`}
        title={title || `${label}: ${valueLabel}`}
        onClick={() => onOpen(!open)}
      >
        <span className="chrome-chip-icon">{icon}</span>
        <span className="chrome-chip-v">{valueLabel}</span>
        <span className="chrome-chip-caret">
          <IconChevron />
        </span>
      </button>
      {open && (
        <ul className="chrome-menu-list" role="listbox">
          {options.map((opt) => (
            <li key={opt.id}>
              <button
                type="button"
                role="option"
                aria-selected={opt.id === value}
                className={opt.id === value ? "active" : ""}
                onClick={() => {
                  onChange(opt.id);
                  onOpen(false);
                }}
              >
                {opt.label}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export default function App() {
  const storedSessions = readStoredSessions();
  const bootSession =
    storedSessions?.sessions?.find((s) => s.id === storedSessions.activeId) ||
    storedSessions?.sessions?.[0] ||
    blankSessionEntry();

  const [mock] = useState(() => isMockMode());
  /** `make mock-ui` / `npm run dev:mock` bake mock in — Exit cannot leave. */
  const mockLocked = import.meta.env.VITE_FREYA_MOCK === "true";
  const [tab, setTab] = useState("gallery");
  const [prompt, setPrompt] = useState(() => bootSession.prompt || "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [result, setResult] = useState(() => bootSession.result ?? null);
  const [batchResults, setBatchResults] = useState(() =>
    Array.isArray(bootSession.results) ? bootSession.results : [],
  );
  const [styles, setStyles] = useState([]);
  const [gallery, setGallery] = useState([]);
  const [settings, setSettings] = useState(null);
  const [civitaiKey, setCivitaiKey] = useState("");
  const [openrouterKey, setOpenrouterKey] = useState("");
  const [studioToken, setStudioToken] = useState(
    () =>
      (typeof localStorage !== "undefined" && localStorage.getItem("freya.apiToken")) || "",
  );
  const [generatedToken, setGeneratedToken] = useState(null);
  const [tokenBusy, setTokenBusy] = useState(false);
  const [tokenCopyMsg, setTokenCopyMsg] = useState("");
  const [setupCmdCopyMsg, setSetupCmdCopyMsg] = useState("");
  const [workerHost, setWorkerHost] = useState(DEFAULT_WORKER_HOST);
  const [workerPort, setWorkerPort] = useState(DEFAULT_WORKER_PORT);
  const [workerUrlOverride, setWorkerUrlOverride] = useState("");
  const [useWorkerUrlOverride, setUseWorkerUrlOverride] = useState(false);
  const [workerStats, setWorkerStats] = useState(null);
  const [workerCtlBusy, setWorkerCtlBusy] = useState(false);
  const [civitaiQ, setCivitaiQ] = useState("");
  const installedFingerprintRef = useRef("");
  const localCheckpointsFpRef = useRef("");
  const downloadJobsFingerprintRef = useRef("");
  const enrichAttemptedRef = useRef(new Set());
  const enrichTimerRef = useRef(null);
  const lastSucceededJobIdsRef = useRef(new Set());
  const [civitaiItems, setCivitaiItems] = useState([]);
  const [savedMsg, setSavedMsg] = useState("");
  const [ideas, setIdeas] = useState(() =>
    Array.isArray(bootSession.ideas) ? bootSession.ideas : [],
  );
  const [suggestedLoras, setSuggestedLoras] = useState(() =>
    Array.isArray(bootSession.suggestedLoras) ? bootSession.suggestedLoras : [],
  );
  const [suggestNote, setSuggestNote] = useState(() => bootSession.suggestNote || "");
  const [civitaiQuery, setCivitaiQuery] = useState(() => bootSession.civitaiQuery || "");
  const [modelBrief, setModelBrief] = useState(() => bootSession.modelBrief || "");
  const [session, setSession] = useState(() =>
    bootSession.session
      ? {
          goal: bootSession.session.goal || "",
          pinnedLoras: Array.isArray(bootSession.session.pinnedLoras)
            ? bootSession.session.pinnedLoras
            : [],
          turns: Array.isArray(bootSession.session.turns) ? bootSession.session.turns : [],
          sessionNote: bootSession.session.sessionNote || "",
        }
      : emptySession(),
  );
  const [automationMode, setAutomationMode] = useState(readStoredMode);
  const storedModels = readStoredModels();
  const [agentModel, setAgentModel] = useState(storedModels.agentModel);
  const [imageModel, setImageModel] = useState(storedModels.imageModel);
  const [videoModel, setVideoModel] = useState(
    typeof storedModels.videoModel === "string" && storedModels.videoModel.trim()
      ? storedModels.videoModel.trim()
      : AUTO_VIDEO.id,
  );
  const [routerModel, setRouterModel] = useState(storedModels.routerModel);
  const [generateBackend, setGenerateBackend] = useState(readGenerateBackend);
  const [localCheckpoints, setLocalCheckpoints] = useState([]);
  const [localBaseModel, setLocalBaseModel] = useState("");
  const [localBaseFamily, setLocalBaseFamily] = useState("");
  const [localBaseBusy, setLocalBaseBusy] = useState(false);
  const [pendingPlan, setPendingPlan] = useState(() => bootSession.pendingPlan ?? null);
  const [pendingQuestion, setPendingQuestion] = useState(
    () => bootSession.pendingQuestion ?? null,
  );
  const [sessionLog, setSessionLog] = useState(() =>
    Array.isArray(bootSession.sessionLog) ? bootSession.sessionLog : [],
  );
  const [menuOpen, setMenuOpen] = useState(null);
  const [theme, setTheme] = useState(resolveInitialTheme);
  const [sidebarOpen, setSidebarOpen] = useState(readStoredSidebarOpen);
  const [activeSessionId, setActiveSessionId] = useState(
    () => bootSession.id || makeSessionId(),
  );
  const [sessions, setSessions] = useState(() =>
    storedSessions?.sessions?.length ? storedSessions.sessions : [bootSession],
  );
  const [confirmDeleteId, setConfirmDeleteId] = useState(null);
  const [archivedOpen, setArchivedOpen] = useState(() => {
    const active = storedSessions?.sessions?.find((s) => s.id === storedSessions.activeId);
    return !!active?.archived;
  });
  const sessionsRef = useRef(sessions);
  const activeIdRef = useRef(activeSessionId);
  const timelineRef = useRef(null);

  useEffect(() => {
    sessionsRef.current = sessions;
  }, [sessions]);

  useEffect(() => {
    activeIdRef.current = activeSessionId;
  }, [activeSessionId]);

  useEffect(() => {
    if (!confirmDeleteId) return;
    const onKey = (e) => {
      if (e.key === "Escape") setConfirmDeleteId(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [confirmDeleteId]);

  // Apply theme to <html data-theme>; persistence happens only on toggle,
  // so an unsaved preference keeps following the system.
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    document.documentElement.style.colorScheme = theme;
    document
      .querySelector('meta[name="theme-color"]')
      ?.setAttribute("content", theme === "dark" ? "#0e1418" : "#f6f1e4");
  }, [theme]);

  // Follow system changes until the user picks a theme explicitly.
  useEffect(() => {
    if (readStoredTheme()) return;
    const mq = window.matchMedia("(prefers-color-scheme: light)");
    const onChange = (e) => {
      // Ignore OS changes after an explicit toggle writes freya.theme.
      if (readStoredTheme()) return;
      setTheme(e.matches ? "light" : "dark");
    };
    mq.addEventListener?.("change", onChange);
    return () => mq.removeEventListener?.("change", onChange);
  }, []);

  const toggleTheme = () => {
    const next = theme === "dark" ? "light" : "dark";
    setTheme(next);
    try {
      localStorage.setItem(THEME_STORAGE_KEY, next);
    } catch {
      /* ignore */
    }
  };

  /** Featured style example from /v1/styles (styles[0]) — same text Styles tab shows. */
  const examplePrompt = styles[0]?.examplePrompt || BEGINNER_EXAMPLE;
  const exampleCivitaiQuery = demoCivitaiQuery(examplePrompt);

  const snapshotCurrent = (id = activeIdRef.current) => {
    const prev = sessionsRef.current.find((s) => s.id === id);
    const now = new Date().toISOString();
    return {
      ...(prev || {}),
      id,
      title: sessionTitleFrom(prompt, session),
      createdAt: prev?.createdAt || now,
      updatedAt: now,
      archived: !!prev?.archived,
      prompt,
      session: {
        goal: session.goal || "",
        pinnedLoras: session.pinnedLoras || [],
        turns: session.turns || [],
        sessionNote: session.sessionNote || "",
      },
      ideas,
      suggestedLoras,
      suggestNote,
      civitaiQuery,
      result,
      results: batchResults,
      modelBrief,
      pendingPlan,
      pendingQuestion,
      sessionLog,
    };
  };

  const applySessionEntry = (entry) => {
    setPrompt(entry.prompt || "");
    setSession(
      entry.session
        ? {
            goal: entry.session.goal || "",
            pinnedLoras: Array.isArray(entry.session.pinnedLoras)
              ? entry.session.pinnedLoras
              : [],
            turns: Array.isArray(entry.session.turns) ? entry.session.turns : [],
            sessionNote: entry.session.sessionNote || "",
          }
        : emptySession(),
    );
    setIdeas(Array.isArray(entry.ideas) ? entry.ideas : []);
    setSuggestedLoras(Array.isArray(entry.suggestedLoras) ? entry.suggestedLoras : []);
    setSuggestNote(entry.suggestNote || "");
    setCivitaiQuery(entry.civitaiQuery || "");
    if (entry.civitaiQuery) setCivitaiQ(entry.civitaiQuery);
    setResult(entry.result ?? null);
    setBatchResults(Array.isArray(entry.results) ? entry.results : []);
    setModelBrief(entry.modelBrief || "");
    setPendingPlan(entry.pendingPlan ?? null);
    setPendingQuestion(entry.pendingQuestion ?? null);
    setSessionLog(Array.isArray(entry.sessionLog) ? entry.sessionLog : []);
    setError("");
    setSavedMsg("");
  };

  const upsertSessionList = (snap, list = sessionsRef.current) => {
    const idx = list.findIndex((s) => s.id === snap.id);
    if (idx === -1) return [snap, ...list];
    const prev = list[idx];
    const next = [...list];
    next[idx] = {
      ...prev,
      ...snap,
      createdAt: prev.createdAt || snap.createdAt,
      archived: snap.archived != null ? !!snap.archived : !!prev.archived,
    };
    return next;
  };

  const persistAll = (activeId, list) => {
    setSessions(list);
    sessionsRef.current = list;
    activeIdRef.current = activeId;
    persistSessionsStore(activeId, list);
  };

  const startNewSession = () => {
    setConfirmDeleteId(null);
    const snap = snapshotCurrent();
    const blank = blankSessionEntry();
    const list = [blank, ...upsertSessionList(snap).filter((s) => s.id !== blank.id)];
    persistAll(blank.id, list);
    setActiveSessionId(blank.id);
    applySessionEntry(blank);
    setTab("gallery");
    setSavedMsg("Started a new session.");
  };

  const switchSession = (id) => {
    if (!id || id === activeSessionId) return;
    const target = sessionsRef.current.find((s) => s.id === id);
    if (!target) return;
    setConfirmDeleteId(null);
    const snap = snapshotCurrent();
    const list = upsertSessionList(snap);
    persistAll(id, list);
    setActiveSessionId(id);
    applySessionEntry(target);
    if (target.archived) setArchivedOpen(true);
    setTab("gallery");
  };

  /** Hide from the active list. Session record stays in localStorage. */
  const setSessionArchived = (id, archived) => {
    if (!id) return;
    setConfirmDeleteId(null);
    const now = new Date().toISOString();
    const currentId = activeIdRef.current;
    let list = sessionsRef.current;
    if (id === currentId) {
      list = upsertSessionList({ ...snapshotCurrent(id), archived: !!archived, updatedAt: now }, list);
    } else {
      list = upsertSessionList(snapshotCurrent(currentId), list).map((s) =>
        s.id === id ? { ...s, archived: !!archived, updatedAt: now } : s,
      );
    }
    if (archived && id === currentId) {
      const fallback = list.find((s) => s.id !== id && !s.archived);
      if (fallback) {
        persistAll(fallback.id, list);
        setActiveSessionId(fallback.id);
        applySessionEntry(fallback);
      } else {
        const blank = blankSessionEntry();
        const next = [blank, ...list.filter((s) => s.id !== blank.id)];
        persistAll(blank.id, next);
        setActiveSessionId(blank.id);
        applySessionEntry(blank);
      }
      setArchivedOpen(true);
      setSavedMsg("Archived.");
      return;
    }
    persistAll(currentId, list);
    if (archived) setArchivedOpen(true);
    setSavedMsg(archived ? "Archived." : "Unarchived.");
  };

  /** Drop the session record only. Gallery images stay; they are not owned by the session. */
  const deleteSession = (id) => {
    if (!id) return;
    setConfirmDeleteId(null);
    const currentId = activeIdRef.current;
    let list = sessionsRef.current.filter((s) => s.id !== id);
    if (id !== currentId) {
      list = upsertSessionList(snapshotCurrent(currentId), list);
      if (!list.length) list = [blankSessionEntry()];
      const nextId = list.some((s) => s.id === currentId) ? currentId : list[0].id;
      persistAll(nextId, list);
      if (nextId !== currentId) {
        const next = list.find((s) => s.id === nextId);
        setActiveSessionId(nextId);
        if (next) applySessionEntry(next);
      }
      setSavedMsg("Deleted.");
      return;
    }
    const fallback = list.find((s) => !s.archived);
    if (fallback) {
      persistAll(fallback.id, list);
      setActiveSessionId(fallback.id);
      applySessionEntry(fallback);
    } else {
      const blank = blankSessionEntry();
      persistAll(blank.id, [blank, ...list]);
      setActiveSessionId(blank.id);
      applySessionEntry(blank);
    }
    setSavedMsg("Deleted.");
  };

  const setSidebar = (open) => {
    setSidebarOpen(open);
    try {
      localStorage.setItem(SIDEBAR_STORAGE_KEY, open ? "1" : "0");
    } catch {
      /* ignore */
    }
  };

  // Keep the active session entry fresh for reload / switch (mock persistence).
  useEffect(() => {
    const snap = snapshotCurrent();
    const list = upsertSessionList(snap);
    sessionsRef.current = list;
    setSessions(list);
    persistSessionsStore(activeSessionId, list);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- intentional sync of live working set
  }, [
    activeSessionId,
    prompt,
    session,
    ideas,
    suggestedLoras,
    suggestNote,
    civitaiQuery,
    result,
    batchResults,
    modelBrief,
    pendingPlan,
    pendingQuestion,
    sessionLog,
  ]);

  const refresh = async () => {
    try {
      const [s, st, g] = await Promise.all([
        api("/v1/styles"),
        api("/v1/settings"),
        api("/v1/gallery"),
      ]);
      const items = s.items || [];
      setStyles(items);
      setSettings(st);
      applyPrivacyAlignmentFromSettings(st);
      applyInstructionsFromSettings(st);
      if (typeof st.chatModel === "string" && st.chatModel.trim()) {
        setAgentModel(st.chatModel.trim());
      }
      if (typeof st.imageModel === "string" && st.imageModel.trim()) {
        setImageModel(st.imageModel.trim());
      }
      if (typeof st.routerModel === "string" && st.routerModel.trim()) {
        setRouterModel(st.routerModel.trim());
      }
      persistModels(
        st.chatModel || agentModel,
        st.imageModel || imageModel,
        st.routerModel || routerModel,
        st.studio?.videoModel || videoModel,
      );
      applyStudioFromSettings(st);
      {
        const ep = parseWorkerEndpoint(st.diffusionWorkerUrl || "");
        setWorkerHost(ep.host);
        setWorkerPort(ep.port);
        setWorkerUrlOverride(ep.advancedUrl);
        setUseWorkerUrlOverride(ep.useAdvanced);
      }
      setGallery(g.items || []);
      setError("");
      setAuthCta("");
    } catch (e) {
      setError(e.message);
    }
    await refreshModels();
  };

  const submitLogin = async (e) => {
    e?.preventDefault?.();
    setLoginBusy(true);
    setLoginError("");
    try {
      const res = await api("/v1/auth/login", {
        method: "POST",
        body: JSON.stringify({ username: loginUser, password: loginPass }),
      });
      const tok = res.token || "";
      localStorage.setItem("freya.apiToken", tok);
      setStudioToken(tok);
      setLoginPass("");
      setShowLogin(false);
      setAuthCta("");
      setSavedMsg(res.note || "Signed in.");
      if (!readGuidelinesAck()) setGuidelinesAcked(false);
      await refresh();
    } catch (err) {
      setLoginError(err.message || "Sign-in failed");
    } finally {
      setLoginBusy(false);
    }
  };

  const acceptGuidelines = () => {
    persistGuidelinesAck();
    setGuidelinesAcked(true);
    // No sticky status under the prompt — Guidelines tab still shows “Acknowledged”.
  };

  const signOut = () => {
    localStorage.removeItem("freya.apiToken");
    setStudioToken("");
    setModelsCatalogReady(false);
    setAgentModels(PLACEHOLDER_AGENT_MODELS);
    setImageModels(PLACEHOLDER_IMAGE_MODELS);
    setRouterModels(PLACEHOLDER_ROUTER_MODELS);
    setShowLogin(true);
    setAuthCta(AUTH_SIGN_IN_HINT);
    setSavedMsg("Signed out.");
  };

  const isFavorite = (item) =>
    favorites.some((f) => loraKey(f) === loraKey(item));

  const toggleFavorite = (item) => {
    setFavorites((prev) => {
      const key = loraKey(item);
      const exists = prev.some((f) => loraKey(f) === key);
      const civitaiId = resolveCivitaiModelId(item);
      const next = exists
        ? prev.filter((f) => loraKey(f) !== key)
        : [
            {
              id: civitaiId ?? item.id,
              modelId: civitaiId ?? item.modelId ?? null,
              versionId: item.versionId ?? null,
              name: item.name,
              modelType: item.modelType,
              downloadUrl: item.downloadUrl,
              thumbnailUrl: item.thumbnailUrl,
              triggerWord: item.triggerWord || item.trainedWords?.[0],
              baseModel: item.baseModel,
              civitaiUrl:
                item.civitaiUrl ||
                (civitaiId ? `https://civitai.com/models/${civitaiId}` : null),
              trainedWords: item.trainedWords,
            },
            ...prev,
          ].slice(0, 200);
      writeFavorites(next);
      void persistStudio({ favorites: next });
      return next;
    });
  };

  const closeCivitaiDetail = () => {
    setCivitaiDetail(null);
    setDetailImageIdx(0);
  };

  const selectCivitaiVariant = (versionId, fileId) => {
    setCivitaiDetail((prev) => {
      if (!prev?.model) return prev;
      const next = applyCivitaiVariant(prev.model, versionId, fileId);
      return { ...prev, model: next };
    });
    setDetailImageIdx(0);
  };

  const openCivitaiDetail = async (item) => {
    if (!item?.id && !item?.name && !item?.path) return;
    setCivitaiDetailBusy(true);
    setDetailImageIdx(0);
    let civitaiId = resolveCivitaiModelId(item);
    const versionId = item.versionId != null && item.versionId !== "" ? item.versionId : null;
    const canFetch = Boolean(settings?.civitaiConfigured);
    const willFetch = canFetch && (civitaiId != null || versionId != null);
    const thumbHint = pickCivitaiThumb(
      { ...item, modelId: civitaiId ?? item.modelId, versionId },
      favorites,
    );
    const initial = applyCivitaiVariant(
      {
        ...item,
        modelId: civitaiId ?? item.modelId,
        thumbnailUrl: thumbHint || item.thumbnailUrl,
        localAsset: Boolean(item.path) && !willFetch,
      },
      item.versionId,
      item.fileId,
    );
    setCivitaiDetail({
      model: initial,
      related: [],
      loading: willFetch,
      localOnly: !willFetch && Boolean(item.path),
    });
    try {
      if (willFetch) {
        let data;
        if (civitaiId != null) {
          try {
            data = await api(`/v1/civitai/models/${civitaiId}`);
          } catch (err) {
            // Chat cards sometimes carry a version id where a model id belongs.
            if (versionId == null) throw err;
            data = await api(`/v1/civitai/model-versions/${versionId}`);
          }
        } else {
          // Installed sidecars often have versionId but null modelId — resolve via version.
          data = await api(`/v1/civitai/model-versions/${versionId}`);
        }
        // Accept nested { model, related } or a flat detail payload.
        const model = data.model || data;
        const related = Array.isArray(data.related)
          ? data.related
          : Array.isArray(model.related)
            ? model.related
            : [];
        const resolvedId = model.id ?? model.modelId ?? civitaiId;
        if (resolvedId != null) civitaiId = resolvedId;
        const merged = applyCivitaiVariant(
          {
            ...item,
            ...model,
            modelId: civitaiId,
            id: civitaiId ?? model.id ?? item.id,
            path: item.path || model.path,
            thumbnailUrl:
              model.thumbnailUrl ||
              model.imageUrls?.[0] ||
              thumbHint ||
              item.thumbnailUrl,
          },
          item.versionId ?? model.versionId,
          item.fileId ?? model.fileId,
        );
        const resolvedThumb = merged.thumbnailUrl || merged.imageUrls?.[0] || null;
        rememberCivitaiThumb(
          { modelId: civitaiId, versionId: merged.versionId ?? versionId },
          resolvedThumb,
        );
        if (item.path && resolvedThumb) {
          setInstalledAssets((prev) =>
            prev.map((a) =>
              a.path === item.path
                ? {
                    ...a,
                    thumbnailUrl: a.thumbnailUrl || resolvedThumb,
                    modelId: a.modelId ?? civitaiId ?? null,
                    imageUrls: a.imageUrls?.length ? a.imageUrls : merged.imageUrls,
                  }
                : a,
            ),
          );
        }
        setCivitaiDetail({
          model: merged,
          related,
          loading: false,
          localOnly: false,
        });
      } else {
        setCivitaiDetail({
          model: initial,
          related: item.related || [],
          loading: false,
          localOnly: Boolean(item.path) && civitaiId == null,
        });
      }
    } catch (e) {
      // Keep the list/suggest card usable if detail fetch misses (e.g. mock catalog vs live id).
      setCivitaiDetail({
        model: initial,
        related: [],
        loading: false,
        localOnly: Boolean(item.path),
        error: item?.name ? null : e.message,
      });
    } finally {
      setCivitaiDetailBusy(false);
    }
  };

  useEffect(() => {
    refresh();
  }, []);

  useEffect(() => {
    const onAuthRequired = () => {
      setAuthCta(AUTH_SIGN_IN_HINT);
      setShowLogin(true);
    };
    window.addEventListener(AUTH_REQUIRED_EVENT, onAuthRequired);
    return () => window.removeEventListener(AUTH_REQUIRED_EVENT, onAuthRequired);
  }, []);

  useEffect(() => {
    if (tab !== "help" || !helpFocus) return undefined;
    const t = window.setTimeout(() => {
      const el = document.getElementById(helpFocus);
      if (el) el.scrollIntoView({ behavior: "smooth", block: "start" });
    }, 40);
    return () => window.clearTimeout(t);
  }, [tab, helpFocus]);

  const refreshWorkerStats = async () => {
    if (!mock && !hasBrowserBearer() && authStatus?.authRequired) {
      setWorkerStats(null);
      return;
    }
    try {
      setWorkerStats(await api("/v1/worker/stats"));
    } catch {
      setWorkerStats(null);
    }
  };

  useEffect(() => {
    if (mock) return undefined;
    // Keep topbar Worker status fresh; poll faster while on the Worker tab.
    refreshWorkerStats();
    const ms = tab === "worker" ? 5000 : 20000;
    const id = setInterval(refreshWorkerStats, ms);
    return () => clearInterval(id);
  }, [tab, mock, studioToken, authStatus?.authRequired]);

  const workerControl = async (action) => {
    if (!["start", "stop"].includes(action)) return;
    setWorkerCtlBusy(true);
    setError("");
    setSavedMsg("");
    try {
      await api(`/v1/worker/${action}`, { method: "POST", body: "{}" });
      const st = await api("/v1/settings");
      setSettings(st);
      applyWorkerEndpoint(st.diffusionWorkerUrl || "");
      await refreshWorkerStats();
      const health = await api("/v1/health").catch(() => null);
      const healthNote = health ? ` · health diffusionWorker=${health.diffusionWorker}` : "";
      const note = {
        stop: `Worker stop requested${healthNote}`,
        start: `Worker start requested${healthNote}`,
      }[action];
      setSavedMsg(note);
    } catch (e) {
      setError(e.message);
    } finally {
      setWorkerCtlBusy(false);
    }
  };

  const persistModels = (
    nextAgent,
    nextImage,
    nextRouter = routerModel,
    nextVideo = videoModel,
  ) => {
    try {
      localStorage.setItem(
        MODELS_STORAGE_KEY,
        JSON.stringify({ agentModel: nextAgent, imageModel: nextImage }),
      );
    } catch {
      /* ignore */
    }
  };

  const persistRouterToApi = async (id) => {
    if (mock) return;
    try {
      const st = await api("/v1/settings", {
        method: "PUT",
        body: JSON.stringify({ routerModel: id }),
      });
      // API flattens SettingsPublic + persisted
      setSettings(st);
    } catch {
      /* offline / unauth — localStorage still holds the pick */
    }
  };

  const persistModelPins = async (patch) => {
    if (mock) return;
    try {
      const st = await api("/v1/settings", {
        method: "PUT",
        body: JSON.stringify(patch),
      });
      setSettings(st);
    } catch {
      /* localStorage still holds the pick */
    }
  };

  const persistStudio = async (patch) => {
    if (mock) return;
    try {
      await api("/v1/studio", {
        method: "PUT",
        body: JSON.stringify(patch),
      });
    } catch {
      /* local state still applies */
    }
  };

  const applyStudioFromSettings = (st) => {
    const s = st?.studio;
    if (!s || typeof s !== "object") return;
    if (Array.isArray(s.favorites)) {
      setFavorites(s.favorites);
      writeFavorites(s.favorites);
    }
    if (Array.isArray(s.pinnedLoras)) {
      setSession((prev) => ({
        ...prev,
        pinnedLoras: s.pinnedLoras.filter(isPinnableAsset),
      }));
    }
    if (s.generateBackend === "local" || s.generateBackend === "cloud") {
      setGenerateBackend(s.generateBackend);
      persistGenerateBackend(s.generateBackend);
    }
    if (typeof s.videoModel === "string" && s.videoModel.trim()) {
      const video = s.videoModel.trim();
      setVideoModel(video);
      // Only the video pin. agent/image/router in this render can be older
      // than the settings response that started the refresh.
      try {
        const raw = localStorage.getItem(MODELS_STORAGE_KEY);
        const parsed = raw ? JSON.parse(raw) : {};
        if (parsed && typeof parsed === "object") {
          localStorage.setItem(
            MODELS_STORAGE_KEY,
            JSON.stringify({ ...parsed, videoModel: video }),
          );
        }
      } catch {
        /* ignore */
      }
    }
    if (s.automationMode === "plan" || s.automationMode === "auto" || s.automationMode === "yolo") {
      setAutomationMode(s.automationMode);
      try {
        localStorage.setItem(AUTOMATION_STORAGE_KEY, s.automationMode);
      } catch {
        /* ignore */
      }
    }
  };

  const setMode = (mode) => {
    if (!AUTOMATION_MODES.includes(mode)) return;
    setAutomationMode(mode);
    try {
      localStorage.setItem(AUTOMATION_STORAGE_KEY, mode);
    } catch {
      /* ignore */
    }
    void persistStudio({ automationMode: mode });
  };

  const changeAgentModel = (id) => {
    setAgentModel(id);
    persistModels(id, imageModel, routerModel, videoModel);
    void persistModelPins({ chatModel: id });
  };

  const changeImageModel = (id) => {
    setImageModel(id);
    persistModels(agentModel, id, routerModel, videoModel);
    void persistModelPins({ imageModel: id });
  };

  const changeVideoModel = (id) => {
    if (!modelsCatalogReady && id === AUTO_VIDEO.id) return;
    setVideoModel(id);
    persistModels(agentModel, imageModel, routerModel, id);
    void persistStudio({ videoModel: id });
  };

  const changeRouterModel = (id) => {
    if (!modelsCatalogReady && id === AUTO_ROUTER.id) return;
    setRouterModel(id);
    persistModels(agentModel, imageModel, id, videoModel);
    void persistRouterToApi(id);
  };

  const composedWorkerUrl = () =>
    useWorkerUrlOverride
      ? workerUrlOverride.trim()
      : composeWorkerUrl(workerHost, workerPort);
  // Prefer persisted URL so default placeholders do not fake a configured worker.
  const hasWorker = () => Boolean(settings?.diffusionWorkerUrl);
  const workerReachable = Boolean(workerStats?.reachable);
  const localBackendOk = hasWorker() && workerReachable;
  const localBackendReason = !hasWorker()
    ? "Set a worker URL on the Worker tab first"
    : !workerReachable
      ? "Worker URL set but unreachable"
      : "";
  const changeGenerateBackend = (next) => {
    if (next === "local" && !localBackendOk) return;
    setGenerateBackend(next);
    persistGenerateBackend(next);
    void persistStudio({ generateBackend: next });
    if (menuOpen === "gen-bundle") setMenuOpen(null);
  };
  const canAutoInstall = () => Boolean(settings?.civitaiConfigured && hasWorker());

  const refreshLocalBases = () => refreshInstalledAssets({ quiet: true });

  useEffect(() => {
    if (generateBackend !== "local" && !hasWorker()) return undefined;
    refreshLocalBases();
    return undefined;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [generateBackend, settings?.diffusionWorkerUrl, workerStats?.reachable]);

  const setComposerBaseModel = async (path) => {
    if (!path?.trim()) return;
    setLocalBaseBusy(true);
    setError("");
    try {
      const match = localCheckpoints.find((c) => c.path === path);
      const res = await api("/v1/worker/base-model", {
        method: "PUT",
        body: JSON.stringify({
          model: path.trim(),
          baseModel: match?.baseModel || undefined,
        }),
      });
      const next = res?.model || path;
      setLocalBaseModel(next);
      setActiveBasePath(next);
      if (typeof res?.baseModel === "string") setLocalBaseFamily(res.baseModel);
      setSavedMsg(`Active base set to ${next}`);
    } catch (e) {
      setError(e.message);
    } finally {
      setLocalBaseBusy(false);
    }
  };

  const applyWorkerEndpoint = (url) => {
    const ep = parseWorkerEndpoint(url || "");
    setWorkerHost(ep.host);
    setWorkerPort(ep.port);
    setWorkerUrlOverride(ep.advancedUrl);
    setUseWorkerUrlOverride(ep.useAdvanced);
  };

  const generateApiToken = async () => {
    setTokenBusy(true);
    setError("");
    setSavedMsg("");
    setTokenCopyMsg("");
    try {
      const res = await api("/v1/auth/token", { method: "POST", body: "{}" });
      const tok = res?.token || "";
      if (!tok) throw new Error("API returned no token");
      setGeneratedToken(tok);
      setStudioToken(tok);
      localStorage.setItem("freya.apiToken", tok);
      await refresh();
      flashSave(
        "token",
        res.note ||
          (res.rotated
            ? "Machine API token rotated — copy it now."
            : "Machine API token created — copy it now."),
      );
    } catch (e) {
      setError(
        e.message === "Unauthorized" || /401|Unauthorized/i.test(e.message || "")
          ? "Sign in first, then generate a machine API token."
          : e.message,
      );
    } finally {
      setTokenBusy(false);
    }
  };

  const copyGeneratedToken = async () => {
    if (!generatedToken) return;
    try {
      await navigator.clipboard.writeText(generatedToken);
      setTokenCopyMsg("Copied");
    } catch {
      setTokenCopyMsg("Select and copy manually");
    }
  };

  /** Prefer freshly generated token, else browser-saved machine token. */
  const effectiveMachineToken = () =>
    (generatedToken || studioToken || "").trim();

  const linuxWorkerSetupCmd = () =>
    buildLinuxWorkerSetupCmd({
      origin: studioOrigin(),
      token: effectiveMachineToken(),
      apiUrl: studioOrigin(),
    });

  const windowsWorkerSetupCmd = () =>
    buildWindowsWorkerSetupCmd({
      origin: studioOrigin(),
      token: effectiveMachineToken(),
      apiUrl: studioOrigin(),
    });

  const copyWorkerSetupCmd = async (which) => {
    const text = which === "windows" ? windowsWorkerSetupCmd() : linuxWorkerSetupCmd();
    try {
      await navigator.clipboard.writeText(text);
      setSetupCmdCopyMsg(which === "windows" ? "Windows copied" : "Linux copied");
    } catch {
      setSetupCmdCopyMsg("Select and copy manually");
    }
  };

  const applyLocalWorkerDefaults = () => {
    setUseWorkerUrlOverride(false);
    setWorkerUrlOverride("");
    setWorkerHost(DEFAULT_WORKER_HOST);
    setWorkerPort(DEFAULT_WORKER_PORT);
    setSavedMsg(
      `Local worker endpoint set to http://${DEFAULT_WORKER_HOST}:${DEFAULT_WORKER_PORT} — Save worker to apply.`,
    );
  };

  const persistStudioTokenLocal = () => {
    if (studioToken.trim()) {
      localStorage.setItem("freya.apiToken", studioToken.trim());
    } else {
      localStorage.removeItem("freya.apiToken");
    }
  };

  /** Persist machine API token in this browser and reload catalogs. */
  const applyStudioToken = async () => {
    persistStudioTokenLocal();
    setError("");
    setSavedMsg("");
    setSaveFlash(null);
    try {
      await refresh();
      flashSave(
        "token",
        studioToken.trim() ? "Machine token saved in this browser." : "Machine token cleared.",
      );
    } catch (e) {
      setError(e.message);
    }
  };

  const saveWorkerEndpoint = async () => {
    setBusy(true);
    setError("");
    setSavedMsg("");
    setSaveFlash(null);
    try {
      persistStudioTokenLocal();
      const st = await api("/v1/settings", {
        method: "PUT",
        body: JSON.stringify({ diffusionWorkerUrl: composedWorkerUrl() }),
      });
      setSettings(st);
      applyWorkerEndpoint(st.diffusionWorkerUrl || "");
      await refreshWorkerStats();
      flashSave(
        "worker",
        mock
          ? "Worker endpoint saved (mock)."
          : `Worker endpoint saved: ${st.diffusionWorkerUrl || "(cleared — OpenRouter only)"}.`,
      );
    } catch (e) {
      setError(e.message);
    } finally {
      setBusy(false);
    }
  };
  const canCloudGenerate = () => Boolean(settings?.openRouterConfigured);

  const pinnedPayload = () =>
    (session.pinnedLoras || []).map((l) => ({
      name: l.name,
      scale: l.scale ?? 0.8,
    }));

  const isPinned = (item) =>
    (session.pinnedLoras || []).some((p) => loraKey(p) === loraKey(item));

  const pinLora = (item) => {
    setSession((prev) => {
      if ((prev.pinnedLoras || []).some((p) => loraKey(p) === loraKey(item))) return prev;
      const next = {
        id: item.id,
        name: item.name,
        modelType: item.modelType,
        downloadUrl: item.downloadUrl,
        triggerWord: item.triggerWord || null,
        scale: 0.8,
      };
      const baseHint = item.baseModel
        ? ` Needs base family “${item.baseModel}” on the local worker (Worker → active base / install checkpoint).`
        : "";
      const pinnedLoras = [...(prev.pinnedLoras || []).filter(isPinnableAsset), next];
      void persistStudio({ pinnedLoras });
      return {
        ...prev,
        pinnedLoras,
        sessionNote: item.triggerWord
          ? `Pinned “${item.name}”. Trigger word “${item.triggerWord}” is optional — Freya keeps your prompt as you wrote it.`
          : `Pinned “${item.name}” for this session. Your prompt stays as you wrote it.`,
      };
    });
    setSavedMsg(`Pinned ${item.name} for this session.`);
  };

  const unpinLora = (item) => {
    setSession((prev) => {
      const pinnedLoras = (prev.pinnedLoras || []).filter((p) => loraKey(p) !== loraKey(item));
      void persistStudio({ pinnedLoras });
      return { ...prev, pinnedLoras };
    });
  };

  const pushTurn = (text, artifactIds = []) => {
    setSession((prev) => ({
      ...prev,
      goal: prev.goal || text,
      turns: [{ prompt: text, at: new Date().toISOString(), artifactIds }, ...(prev.turns || [])].slice(
        0,
        12,
      ),
    }));
  };

  const appendMessages = (...msgs) => {
    setSession((prev) => ({
      ...prev,
      messages: [...(prev.messages || []), ...msgs].slice(-40),
    }));
  };

  const applyChatResponse = (data, userText) => {
    const actions = Array.isArray(data.actions) ? data.actions : [];
    const artifacts = Array.isArray(data.artifacts)
      ? data.artifacts.filter((a) => a && (a.artifactId || a.url))
      : [];
    const artifactIds = artifacts.map((a) => a.artifactId).filter(Boolean);
    const ideasNext = Array.isArray(data.ideas) ? data.ideas : [];
    const query = data.civitaiQuery || "";
    const sourceCards = cardsFromChatPayload(data);
    const presented = presentLookup(sourceCards, installedAssets, query || "");
    const lorasNext = [...presented.existing, ...presented.fresh];

    if (ideasNext.length) setIdeas(ideasNext);
    if (lorasNext.length) {
      const fromSearch = sourceCards.some((c) => c.fromChatSearch);
      // Search payload keeps version ids so installed rows can match them.
      // A worker listing is stored only after that lookup filter.
      setSuggestedLoras(fromSearch ? sourceCards : lorasNext);
      // Search results are browsed on Styles. A turn that also made images stays put.
      if (!artifacts.length) setTab("styles");
    }
    if (settings?.civitaiConfigured) {
      const bare = presented.existing.filter(
        (a) => a?.path && !a.thumbnailUrl && (a.versionId != null || a.modelId != null),
      );
      if (bare.length) scheduleEnrichInstalledThumbs(bare);
    }
    if (query) {
      setCivitaiQuery(query);
      setCivitaiQ(query);
    }

    if (artifacts.length) {
      setBatchResults(artifacts.length > 1 ? artifacts : []);
      setResult(artifacts[0]);
      setModelBrief(artifacts[0]?.enhancedPrompt || "");
    }

    const proposed = actions.filter((a) => a.status === "proposed");
    if (data.pendingConfirmation && proposed.length) {
      setPendingPlan({
        status: automationMode === "plan" ? "plan" : "auto",
        goal: userText,
        actions: proposed,
        batchPrompts: proposed
          .filter((a) => a.type === "generate_image" || a.type === "run_session")
          .map((a) => a.args?.prompt || a.args?.goal)
          .filter(Boolean),
        pendingInstalls: proposed
          .filter((a) => a.type === "install_asset")
          .map((a) => ({
            name: a.args?.name,
            downloadUrl: a.args?.downloadUrl,
            destPath: a.args?.destPath,
          })),
        steps: proposed.map((a) => ({
          kind: a.type,
          note: a.args?.prompt || a.args?.query || a.args?.goal || a.type,
        })),
        civitaiQuery: query,
        sessionNote: data.reply || "",
      });
    } else {
      setPendingPlan(null);
    }

    if (data.question) {
      setPendingQuestion({
        kind: data.question.kind || "tool",
        text: data.question.text || "Freya needs a decision before continuing.",
        choices: data.question.choices || [
          { id: "confirm", label: "Confirm & run", value: "confirm" },
          { id: "skip", label: "Skip tools", value: "skip" },
        ],
      });
    } else if (!data.pendingConfirmation) {
      setPendingQuestion(null);
    }

    const toolLines = actions
      .filter((a) => a.status === "executed" || a.status === "error" || a.status === "proposed")
      .map((a) => {
        if (a.status === "proposed") return `Proposed: ${a.type}`;
        if (a.status === "error") return `${a.type} failed: ${a.error || "error"}`;
        if (a.type === "generate_image") return `Generated image`;
        if (a.type === "run_session")
          return `Generated ${(a.result?.results || []).length || "a"} variation(s)`;
        if (a.type === "search_civitai") {
          const searchedType = a.result?.type || a.args?.type;
          const typeLabel = searchedType ? `${searchedType} ` : "";
          return `Searched Civitai ${typeLabel}for “${a.result?.query || a.args?.query || "…"}”`;
        }
        if (a.type === "suggest_ideas")
          return `Suggested ${(a.result?.ideas || []).length || "some"} ideas`;
        if (a.type === "install_asset") {
          // Queued downloads are signaled by the Downloads label color cycle — no status line.
          if (a.result?.jobId) return null;
          return `Installed “${a.args?.name || "asset"}”`;
        }
        if (a.type === "worker_status")
          return `Worker ${
            a.result?.reachable
              ? "reachable"
              : a.result?.workerConfigured
                ? "unreachable"
                : "not configured"
          }`;
        if (a.type === "list_checkpoints")
          return `Listed ${(a.result?.checkpoints || []).length || 0} checkpoint(s)`;
        if (a.type === "get_base_model")
          return `Active base: ${a.result?.model || "—"}${
            a.result?.baseModel ? ` (${a.result.baseModel})` : ""
          }`;
        if (a.type === "set_base_model")
          return `Set active base to ${a.result?.model || a.args?.model || "—"}`;
        if (a.type === "generate_video") return `Generated ${a.result?.modality || "video"}`;
        if (a.type === "gallery_list")
          return `Gallery: ${(a.result?.items || []).length || 0} item(s)`;
        if (a.type === "gallery_get") return `Gallery item ${a.result?.id || a.args?.id || "—"}`;
        if (a.type === "list_styles")
          return `Listed ${(a.result?.items || []).length || 0} style(s)`;
        if (a.type === "apply_style")
          return `Style ${a.result?.displayName || a.args?.styleRef || "—"}`;
        if (a.type === "list_models") return `Listed models`;
        if (a.type === "get_settings") return `Read settings`;
        if (a.type === "get_route_defaults") return `Route defaults`;
        if (a.type === "set_chat_models")
          return `Saved models (image=${a.result?.imageModel || a.args?.imageModel || "—"})`;
        if (a.type === "openrouter_metrics") return `OpenRouter metrics`;
        return `${a.type}: ${a.status}`;
      })
      .filter(Boolean);
    if (toolLines.length) setSessionLog(toolLines);

    for (const a of actions) {
      if (a.status !== "executed") continue;
      if (
        (a.type === "favorite_asset" || a.type === "unfavorite_asset" || a.type === "list_favorites") &&
        Array.isArray(a.result?.favorites)
      ) {
        setFavorites(a.result.favorites);
        writeFavorites(a.result.favorites);
      }
      if (
        (a.type === "pin_lora" || a.type === "unpin_lora" || a.type === "list_pins") &&
        Array.isArray(a.result?.pinnedLoras)
      ) {
        const pins = a.result.pinnedLoras.filter(isPinnableAsset);
        setSession((prev) => ({ ...prev, pinnedLoras: pins }));
      }
      if (a.type === "set_generate_backend" && (a.result?.backend === "local" || a.result?.backend === "cloud")) {
        setGenerateBackend(a.result.backend);
        persistGenerateBackend(a.result.backend);
      }
      if (
        a.type === "set_automation_mode" &&
        ["plan", "auto", "yolo"].includes(a.result?.automationMode)
      ) {
        setAutomationMode(a.result.automationMode);
        try {
          localStorage.setItem(AUTOMATION_STORAGE_KEY, a.result.automationMode);
        } catch {
          /* ignore */
        }
      }
      if (a.type === "set_chat_models") {
        const chat = a.result?.chatModel;
        const image = a.result?.imageModel;
        const routerM = a.result?.routerModel;
        const video = a.result?.studio?.videoModel;
        if (typeof chat === "string" && chat) setAgentModel(chat);
        if (typeof image === "string" && image) setImageModel(image);
        if (typeof routerM === "string" && routerM) setRouterModel(routerM);
        if (typeof video === "string" && video) setVideoModel(video);
        persistModels(chat || agentModel, image || imageModel, routerM || routerModel, video || videoModel);
        setSettings((prev) => ({ ...(prev || {}), ...a.result }));
      }
      if (a.type === "set_privacy" || a.type === "set_alignment" || a.type === "set_cost_tier") {
        applyPrivacyAlignmentFromSettings(a.result);
        setSettings((prev) => ({ ...(prev || {}), ...a.result }));
      }
      if (a.type === "set_base_model" && a.result?.model) {
        setActiveBasePath(a.result.model);
        setLocalBaseModel(a.result.model);
        if (typeof a.result.baseModel === "string") setLocalBaseFamily(a.result.baseModel);
      }
      if (a.type === "delete_installed") {
        void refreshInstalledAssets({ quiet: true });
      }
      if (a.type === "install_asset") {
        if (a.result?.jobId) void refreshDownloadJobs();
        else if (a.status === "executed") void refreshInstalledAssets({ quiet: true });
      }
      if (a.type === "gallery_delete" && a.result?.id) {
        const id = a.result.id;
        setGallery((prev) => prev.filter((g) => g.id !== id && g.artifactId !== id));
        setBatchResults((prev) => prev.filter((r) => r.artifactId !== id));
        setResult((prev) => (prev?.artifactId === id ? null : prev));
      }
    }

    const zdrErr = actions.find(
      (a) =>
        a.status === "error" &&
        (a.type === "generate_image" || a.type === "generate_video") &&
        /zdr|zero data retention|data-policy|data policy|no endpoints matching/i.test(
          String(a.error || ""),
        ),
    );
    if (zdrErr?.error) {
      setError(
        `${zdrErr.error} — Freya Settings → Privacy (Zero Data Retention), or https://openrouter.ai/settings/privacy`,
      );
    }

    const route = data.route || null;
    const routeContext = data.routeContext || null;
    const resolvedModels = data.resolvedModels || null;
    const genBackend =
      artifacts.find((a) => a?.backend)?.backend ||
      actions.find(
        (a) =>
          (a.type === "generate_image" || a.type === "generate_video") &&
          a.status === "executed" &&
          a.result?.backend,
      )?.result?.backend ||
      null;
    const at = new Date().toISOString();
    appendMessages({
      role: "assistant",
      content: data.reply || (artifacts.length ? "Done." : "…"),
      at,
      actions,
      artifactIds,
      ideas: ideasNext,
      loras: lorasNext,
      civitaiQuery: query || null,
      route,
      routeContext,
      resolvedModels,
      genBackend: genBackend || routeContext?.backend || null,
    });

    if (artifactIds.length) {
      setSession((prev) => ({
        ...prev,
        goal: prev.goal || userText,
        turns: [
          { prompt: userText, at, artifactIds },
          ...(prev.turns || []),
        ].slice(0, 12),
      }));
    }

    return artifacts;
  };

  const chatBody = (message, extra = {}) => {
    const body = {
      message,
      automationMode,
      backend: generateBackend === "local" ? "local" : "openrouter",
      messages: (session.messages || []).slice(-8).map((m) => ({
        role: m.role,
        content: m.content,
      })),
      session: {
        goal: session.goal || message,
        pinnedLoras: session.pinnedLoras,
        recentPrompts: (session.turns || []).slice(0, 5).map((t) => t.prompt),
      },
      loras: pinnedPayload(),
      ...extra,
    };
    body.videoModel = videoModel || AUTO_VIDEO.id;
    if (agentModel && agentModel !== AUTO_AGENT.id) body.model = agentModel;
    if (
      generateBackend !== "local" &&
      imageModel &&
      imageModel !== AUTO_IMAGE.id
    ) {
      body.imageModel = imageModel;
    }
    return body;
  };

  /** Primary composer action — send a chat message; tools run per Plan/Auto/YOLO. */
  const submitChat = async (overrideText, extra = {}) => {
    const text = (overrideText ?? prompt).trim();
    if (!text && !extra.confirm) return;
    if (generateBackend === "local" && !localBackendOk) {
      setError(localBackendReason || "Local worker unavailable");
      return;
    }
    const sessionId = activeIdRef.current;
    const userText = text || session.goal || examplePrompt;
    setBusy(true);
    setError("");
    setSavedMsg("");
    try {
      if (!extra.confirm) {
        const at = new Date().toISOString();
        appendMessages({ role: "user", content: userText, at });
        setSession((prev) => ({ ...prev, goal: prev.goal || userText }));
        setPrompt("");
      }
      const data = await api("/v1/chat", {
        method: "POST",
        body: JSON.stringify(chatBody(userText, extra)),
      });
      if (activeIdRef.current !== sessionId) return;
      applyChatResponse(data, userText);
      const g = await api("/v1/gallery");
      if (activeIdRef.current !== sessionId) return;
      setGallery(g.items || []);
    } catch (e) {
      if (activeIdRef.current === sessionId) setError(e.message);
    } finally {
      if (activeIdRef.current === sessionId) setBusy(false);
    }
  };

  /** Same generate path as asking in chat — uses the turn's prompt, not a retype. */
  const runGenerateOffer = async (offer) => {
    if (!offer?.prompt) return;
    const type = offer.type || "generate_image";
    const action =
      type === "run_session"
        ? { type, args: { goal: offer.prompt } }
        : type === "generate_video"
          ? { type, args: { prompt: offer.prompt, modality: offer.modality || "video" } }
          : { type: "generate_image", args: { prompt: offer.prompt, batch: false } };
    await submitChat(offer.prompt, { confirm: true, actions: [action] });
  };

  const runBody = (nlPrompt) => {
    const loras = pinnedPayload();
    // Pinned LoRAs only apply on the local worker — prefer local when a worker is set.
    const body = {
      prompt: nlPrompt,
      enhance: "auto",
      backend: loras.length && hasWorker() ? "local" : "auto",
      loras,
      automationMode,
    };
    if (imageModel && imageModel !== "auto") body.model = imageModel;
    return body;
  };

  const assistBody = (mode, message) => {
    const body = {
      mode,
      message,
      automationMode,
      session: {
        goal: session.goal || message,
        pinnedLoras: session.pinnedLoras,
        recentPrompts: (session.turns || []).slice(0, 5).map((t) => t.prompt),
      },
    };
    if (agentModel && agentModel !== "openrouter/auto") body.model = agentModel;
    return body;
  };

  const runOne = async (nlPrompt) => {
    const data = await api("/v1/run", {
      method: "POST",
      body: JSON.stringify(runBody(nlPrompt)),
    });
    return data;
  };

  const destPathForAsset = (item) => {
    const safe = (item.name || "model").replace(/[^a-zA-Z0-9._-]+/g, "_");
    const ext = fileExtFromVariant(item);
    const t = (item.modelType || "").toLowerCase();
    if (t === "lora" || t === "locon" || t === "dora" || t === "lycoris" || t.includes("lora")) {
      return `loras/${safe}${ext}`;
    }
    if (t === "textualinversion") return `embeddings/${safe}${ext}`;
    if (t === "vae") return `vae/${safe}${ext}`;
    return `checkpoints/${safe}${ext}`;
  };

  /** Match card/detail selection to an installed worker asset (path preferred). */
  const findInstalledForItem = (item) => {
    if (!item || !installedAssets.length) return null;
    const dest = destPathForAsset(item);
    const byDest = installedAssets.find((a) => a?.path && a.path === dest);
    if (byDest) return byDest;
    if (item.path) {
      const byPath = installedAssets.find((a) => a?.path && a.path === item.path);
      if (byPath) return byPath;
    }
    if (item.versionId != null && item.versionId !== "") {
      const byVer = installedAssets.find(
        (a) => a?.versionId != null && String(a.versionId) === String(item.versionId),
      );
      if (byVer) return byVer;
    }
    return null;
  };

  const activeJobForItem = (item) => {
    if (!item || !downloadJobs.length) return null;
    const dest = destPathForAsset(item);
    return (
      downloadJobs.find((j) => {
        const st = String(j?.status || "").toLowerCase();
        if (!["queued", "downloading"].includes(st)) return false;
        const jp = j?.destPath || j?.path || "";
        return jp === dest || (item.path && jp === item.path);
      }) || null
    );
  };

  const formatBytes = (n) => {
    if (n == null || Number.isNaN(Number(n))) return "";
    const v = Number(n);
    if (v < 1024) return `${v} B`;
    if (v < 1024 * 1024) return `${(v / 1024).toFixed(1)} KB`;
    if (v < 1024 * 1024 * 1024) return `${(v / (1024 * 1024)).toFixed(1)} MB`;
    return `${(v / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  };

  const refreshDownloadJobs = async () => {
    if (!hasWorker()) {
      if (downloadJobsFingerprintRef.current !== "") {
        downloadJobsFingerprintRef.current = "";
        setDownloadJobs([]);
      }
      return [];
    }
    try {
      const data = await api("/v1/civitai/jobs");
      const jobs = Array.isArray(data?.data) ? data.data : [];
      const fp = downloadJobsFingerprint(jobs);
      if (fp !== downloadJobsFingerprintRef.current) {
        downloadJobsFingerprintRef.current = fp;
        setDownloadJobs(jobs);
      }
      return jobs;
    } catch {
      return downloadJobs;
    }
  };

  const refreshInstalledAssets = async ({ quiet = false } = {}) => {
    if (!hasWorker()) {
      if (installedFingerprintRef.current !== "") {
        installedFingerprintRef.current = "";
        setInstalledAssets([]);
      }
      if (localCheckpointsFpRef.current !== "") {
        localCheckpointsFpRef.current = "";
        setLocalCheckpoints([]);
      }
      setActiveBasePath("");
      setLocalBaseModel("");
      setLocalBaseFamily("");
      return;
    }
    if (!quiet) setInstalledBusy(true);
    try {
      const [loras, cps, embeddings, vaes, base] = await Promise.all([
        api("/v1/worker/loras"),
        api("/v1/worker/checkpoints"),
        api("/v1/worker/embeddings"),
        api("/v1/worker/vae"),
        api("/v1/worker/base-model"),
      ]);
      const list = [
        ...(Array.isArray(loras?.data) ? loras.data : []),
        ...(Array.isArray(cps?.data) ? cps.data : []),
        ...(Array.isArray(embeddings?.data) ? embeddings.data : []),
        ...(Array.isArray(vaes?.data) ? vaes.data : []),
      ].map((a) => {
        const thumb = pickCivitaiThumb(
          { ...a, modelId: a.modelId ?? null, versionId: a.versionId ?? null },
          favorites,
        );
        return thumb ? { ...a, thumbnailUrl: thumb } : a;
      });
      const fp = installedListFingerprint(list);
      if (fp !== installedFingerprintRef.current) {
        installedFingerprintRef.current = fp;
        setInstalledAssets(list);
      }
      const cpList = Array.isArray(cps?.data) ? cps.data : [];
      const cpFp = installedListFingerprint(cpList);
      if (cpFp !== localCheckpointsFpRef.current) {
        localCheckpointsFpRef.current = cpFp;
        setLocalCheckpoints(cpList);
      }
      const baseModelPath = typeof base?.model === "string" ? base.model : "";
      const baseFamily = typeof base?.baseModel === "string" ? base.baseModel : "";
      setActiveBasePath(baseModelPath);
      setLocalBaseFamily(baseFamily);
      setLocalBaseModel(baseModelPath);
      // Older sidecars often lack thumbnailUrl — resolve via Civitai when possible.
      if (settings?.civitaiConfigured) {
        scheduleEnrichInstalledThumbs(list);
      }
    } catch (e) {
      if (!quiet) setError(e.message || "Failed to load installed assets");
    } finally {
      if (!quiet) setInstalledBusy(false);
    }
  };

  /** Debounced thumb enrich — skip paths already attempted this session. */
  const scheduleEnrichInstalledThumbs = (list) => {
    if (enrichTimerRef.current) clearTimeout(enrichTimerRef.current);
    enrichTimerRef.current = setTimeout(() => {
      enrichTimerRef.current = null;
      void enrichInstalledThumbs(list);
    }, 400);
  };

  /** Fill missing Installed card thumbs from Civitai model / model-versions. */
  const enrichInstalledThumbs = async (list) => {
    const need = (list || []).filter((a) => {
      if (!a?.path || a.thumbnailUrl) return false;
      if (enrichAttemptedRef.current.has(a.path)) return false;
      return a.versionId != null || (a.modelId != null && a.modelId !== "");
    });
    if (!need.length) return;
    for (const a of need) enrichAttemptedRef.current.add(a.path);
    const concurrency = Math.min(3, need.length);
    let cursor = 0;
    const worker = async () => {
      while (cursor < need.length) {
        const asset = need[cursor++];
        try {
          let data;
          if (asset.modelId != null && asset.modelId !== "") {
            data = await api(`/v1/civitai/models/${asset.modelId}`);
          } else {
            data = await api(`/v1/civitai/model-versions/${asset.versionId}`);
          }
          const model = data?.model || data;
          const thumb = model?.thumbnailUrl || model?.imageUrls?.[0] || null;
          if (!thumb) continue;
          const resolvedModelId = model?.id ?? model?.modelId ?? asset.modelId ?? null;
          rememberCivitaiThumb(
            {
              modelId: resolvedModelId,
              versionId: asset.versionId ?? model?.versionId,
            },
            thumb,
          );
          const applyThumb = (a) =>
            a?.path === asset.path
              ? {
                  ...a,
                  thumbnailUrl: a.thumbnailUrl || thumb,
                  modelId: a.modelId ?? resolvedModelId,
                  imageUrls: a.imageUrls?.length ? a.imageUrls : model?.imageUrls,
                }
              : a;
          setInstalledAssets((prev) => {
            const next = prev.map(applyThumb);
            installedFingerprintRef.current = installedListFingerprint(next);
            return next;
          });
          setSuggestedLoras((prev) => prev.map(applyThumb));
          setSession((prev) => ({
            ...prev,
            messages: (prev.messages || []).map((m) =>
              Array.isArray(m.loras) ? { ...m, loras: m.loras.map(applyThumb) } : m,
            ),
          }));
        } catch {
          /* ignore per-asset resolve failures */
        }
      }
    };
    await Promise.all(Array.from({ length: concurrency }, () => worker()));
  };

  const cancelDownloadJob = async (id) => {
    if (!id) return;
    try {
      await api(`/v1/civitai/jobs/${encodeURIComponent(id)}/cancel`, {
        method: "POST",
      });
      await refreshDownloadJobs();
    } catch (e) {
      setError(e.message);
    }
  };

  /** Clear finished jobs (succeeded/failed/cancelled). Does not cancel in-flight downloads. */
  const clearFinishedDownloadJobs = async () => {
    if (!hasWorker()) return;
    const finished = downloadJobs.filter((j) =>
      ["succeeded", "failed", "cancelled"].includes(String(j?.status || "").toLowerCase()),
    );
    if (finished.length === 0) return;
    try {
      const data = await api("/v1/civitai/jobs/clear", {
        method: "POST",
        body: JSON.stringify({ includeQueued: false }),
      });
      const jobs = Array.isArray(data?.data) ? data.data : [];
      downloadJobsFingerprintRef.current = downloadJobsFingerprint(jobs);
      setDownloadJobs(jobs);
    } catch (e) {
      setError(e.message || "Failed to clear downloads");
    }
  };

  const deleteInstalledAsset = async (asset) => {
    if (!asset?.path) return;
    if (
      !window.confirm(
        `Delete ${asset.name || asset.path} from the worker disk? This cannot be undone.`,
      )
    ) {
      return;
    }
    try {
      await api("/v1/worker/assets", {
        method: "DELETE",
        body: JSON.stringify({ path: asset.path }),
      });
      if (isPinned(asset)) unpinLora(asset);
      await refreshInstalledAssets();
    } catch (e) {
      setError(e.message);
    }
  };

  const setInstalledAsBase = async (asset) => {
    if (!asset?.path) return;
    try {
      await api("/v1/worker/base-model", {
        method: "PUT",
        body: JSON.stringify({
          model: asset.path,
          baseModel: asset.baseModel || undefined,
        }),
      });
      setActiveBasePath(asset.path);
      setLocalBaseModel(asset.path);
      if (typeof asset.baseModel === "string") setLocalBaseFamily(asset.baseModel);
      setSavedMsg(`Active base → ${asset.name || asset.path}`);
    } catch (e) {
      setError(e.message);
    }
  };

  const installOne = async (item) => {
    const safe = (item.name || "model").replace(/[^a-zA-Z0-9._-]+/g, "_");
    const dest =
      item.modelType === "LORA" || item.modelType === "LoCon"
        ? `loras/${safe}.safetensors`
        : `checkpoints/${safe}.safetensors`;
    await api("/v1/civitai/install", {
      method: "POST",
      body: JSON.stringify({
        downloadUrl: item.downloadUrl,
        destPath: dest,
        name: item.name,
      }),
    });
    pinLora(item);
    return dest;
  };

  const executeBatch = async (batch, goal) => {
    const sessionId = activeIdRef.current;
    const results = [];
    for (const bp of batch) {
      const data = await runOne(bp);
      if (activeIdRef.current !== sessionId) return results;
      results.push({ ...data, prompt: bp });
    }
    if (activeIdRef.current !== sessionId) return results;
    setBatchResults(results);
    setResult(results[0] || null);
    setModelBrief(results[0]?.enhancedPrompt || "");
    setPrompt(batch[0] || goal);
    pushTurn(goal, results.map((r) => r.artifactId).filter(Boolean));
    const g = await api("/v1/gallery");
    if (activeIdRef.current !== sessionId) return results;
    setGallery(g.items || []);
    return results;
  };

  const executePendingInstalls = async (loras, _opts = {}) => {
    const log = [];
    const list = installableLoras(loras);
    if (!list.length) return log;

    if (!canAutoInstall()) {
      const names = list.map((l) => l.name).join(", ");
      // Worker is optional — skip install and keep generating on OpenRouter when possible.
      const note = hasWorker()
        ? `Would install ${names} — add a Civitai key in Settings to install.`
        : canCloudGenerate()
          ? `LoRAs need a worker; generating with OpenRouter. (Skipped install: ${names})`
          : `LoRAs need a worker URL to install (${names}). Add a worker for custom/local looks, or configure OpenRouter to generate in the cloud.`;
      log.push(note);
      setSavedMsg(note);
      return log;
    }

    for (const item of list) {
      try {
        const dest = await installOne(item);
        log.push(`Installed “${item.name}” → ${dest}`);
      } catch (e) {
        log.push(`Install skipped for “${item.name}”: ${e.message}`);
      }
    }
    return log;
  };

  const applyAssistPlan = (assist, message) => {
    const goal = assist.goal || message;
    const batch =
      Array.isArray(assist.batchPrompts) && assist.batchPrompts.length
        ? assist.batchPrompts
        : [
            `${message} — wide establishing shot.`,
            `${message} — closer framing.`,
            `${message} — cooler, quieter mood.`,
          ];
    const loras = Array.isArray(assist.loras) ? assist.loras : [];
    const pendingInstalls = installableLoras(loras).slice(0, 3);

    setSession((prev) => ({
      ...prev,
      goal,
      sessionNote: assist.sessionNote || prev.sessionNote,
    }));
    setSuggestedLoras(loras);
    setIdeas([]);
    setSuggestNote(assist.sessionNote || "");
    setCivitaiQuery(assist.civitaiQuery || "");
    if (assist.civitaiQuery) setCivitaiQ(assist.civitaiQuery);

    return {
      goal,
      batchPrompts: batch,
      steps: Array.isArray(assist.steps) ? assist.steps : [],
      civitaiQuery: assist.civitaiQuery || "",
      sessionNote: assist.sessionNote || "",
      loras,
      pendingInstalls,
    };
  };

  const run = async () => {
    const sessionId = activeIdRef.current;
    setBusy(true);
    setError("");
    setBatchResults([]);
    try {
      const data = await runOne(prompt);
      if (activeIdRef.current !== sessionId) return;
      setResult(data);
      setModelBrief(data.enhancedPrompt || "");
      pushTurn(prompt, data.artifactId ? [data.artifactId] : []);
      const g = await api("/v1/gallery");
      if (activeIdRef.current !== sessionId) return;
      setGallery(g.items || []);
    } catch (e) {
      if (activeIdRef.current === sessionId) setError(e.message);
    } finally {
      if (activeIdRef.current === sessionId) setBusy(false);
    }
  };

  /** Ask Freya for NL alternatives + LoRA recommendations (never auto-installs). */
  const suggestIdeas = async () => {
    setBusy(true);
    setError("");
    try {
      const message = prompt.trim() || examplePrompt;
      if (!prompt.trim()) setPrompt(examplePrompt);
      const data = await api("/v1/assist", {
        method: "POST",
        body: JSON.stringify(assistBody("suggest", message)),
      });
      const nextIdeas = Array.isArray(data.ideas)
        ? data.ideas
        : typeof data.text === "string"
          ? data.text
              .split("\n")
              .map((l) => l.replace(/^\d+\.\s*/, "").trim())
              .filter(Boolean)
          : [];
      setIdeas(nextIdeas);
      setSuggestedLoras(Array.isArray(data.loras) ? data.loras : []);
      setSuggestNote(data.sessionNote || "");
      setCivitaiQuery(data.civitaiQuery || "");
      if (data.civitaiQuery) setCivitaiQ(data.civitaiQuery);
      if (data.sessionNote) {
        setSession((prev) => ({ ...prev, sessionNote: data.sessionNote }));
      }
      if (!settings?.civitaiConfigured && (!data.loras || data.loras.length === 0)) {
        setSavedMsg(
          "LoRA suggestions need a Civitai key — add any non-empty value in Settings (mock).",
        );
      }
    } catch (e) {
      setError(e.message);
    } finally {
      setBusy(false);
    }
  };

  const fetchSessionPlan = async () => {
    const message = prompt.trim() || session.goal || examplePrompt;
    if (!prompt.trim() && !session.goal) setPrompt(examplePrompt);
    const assist = await api("/v1/assist", {
      method: "POST",
      body: JSON.stringify(assistBody("session", message)),
    });
    return applyAssistPlan(assist, message);
  };

  /** Detect Auto-mode questions (ambiguous / high-impact). */
  const detectAutoQuestion = (plan) => {
    if (!prompt.trim() && !session.goal) {
      return {
        kind: "goal",
        text: "Your goal is still vague — use the style example, or type a clearer scene before Freya generates?",
        choices: [
          { id: "example", label: "Use example", value: examplePrompt },
          { id: "as-is", label: "Continue with current text", value: plan.goal },
        ],
      };
    }
    // Only ask about installs when a worker can actually receive them.
    if (plan.pendingInstalls?.length > 1 && canAutoInstall()) {
      return {
        kind: "lora",
        text: "Several LoRAs match — which should Freya install before generating?",
        choices: [
          ...plan.pendingInstalls.map((l) => ({
            id: loraKey(l),
            label: `Install “${l.name}”`,
            value: l,
          })),
          { id: "none", label: "Skip installs — generate only", value: null },
          {
            id: "all",
            label: `Install all ${plan.pendingInstalls.length}`,
            value: "all",
          },
        ],
      };
    }
    return null;
  };

  const executePlan = async (plan, { installs = null, yolo = false } = {}) => {
    const log = [];
    if (plan.civitaiQuery) {
      log.push(`Searched Civitai for “${plan.civitaiQuery}”`);
    }

    let toInstall = [];
    if (installs === "all") toInstall = plan.pendingInstalls || [];
    else if (Array.isArray(installs)) toInstall = installs;
    else if (installs && typeof installs === "object") toInstall = [installs];
    else if (yolo) toInstall = plan.pendingInstalls || [];

    if (toInstall.length) {
      const installLog = await executePendingInstalls(toInstall);
      log.push(...installLog);
    } else if ((plan.pendingInstalls || []).length) {
      log.push(
        yolo
          ? `Skipped LoRA installs (${plan.pendingInstalls.map((l) => l.name).join(", ")})`
          : `LoRA installs left for you (${plan.pendingInstalls.map((l) => l.name).join(", ")})`,
      );
    }

    log.push(`Generating ${plan.batchPrompts.length} variations…`);
    await executeBatch(plan.batchPrompts, plan.goal);
    log.push(`Generated ${plan.batchPrompts.length} images for “${plan.goal}”.`);
    if (agentModel && agentModel !== "openrouter/auto") {
      log.push(`Agent model: ${modelLabel(AGENT_MODELS, agentModel)}`);
    }
    if (imageModel && imageModel !== "auto") {
      log.push(`Image model: ${modelLabel(IMAGE_MODELS, imageModel)}`);
    }
    setSessionLog(log);
    setPendingPlan(null);
    setPendingQuestion(null);
    setSuggestNote(
      yolo
        ? plan.sessionNote || "YOLO session complete — Freya ran without pausing."
        : plan.sessionNote || "Session complete. Pin a LoRA or tweak a prompt to iterate.",
    );
  };

  /** Agentic session — gated by Plan / Auto / YOLO. */
  const runSession = async () => {
    const sessionId = activeIdRef.current;
    setBusy(true);
    setError("");
    setBatchResults([]);
    setSessionLog([]);
    setPendingQuestion(null);
    try {
      const plan = await fetchSessionPlan();
      if (activeIdRef.current !== sessionId) return;

      if (automationMode === "plan") {
        setPendingPlan({ ...plan, status: "plan" });
        setSuggestNote(
          plan.sessionNote ||
            "Plan ready — no generate or install. Run pieces manually, or switch to Auto/YOLO and Run session again.",
        );
        setSavedMsg("Plan mode — nothing executed yet.");
        return;
      }

      if (automationMode === "auto") {
        const q = detectAutoQuestion(plan);
        if (q) {
          setPendingPlan({ ...plan, status: "auto" });
          setPendingQuestion(q);
          setSuggestNote(plan.sessionNote || "Freya paused — answer below to continue.");
          setSavedMsg("Auto mode — waiting on your answer.");
          return;
        }
        setPendingPlan(null);
        await executePlan(plan, {
          installs: (plan.pendingInstalls || []).length === 1 ? plan.pendingInstalls : [],
          yolo: false,
        });
        return;
      }

      // YOLO
      setPendingPlan(null);
      await executePlan(plan, { installs: "all", yolo: true });
    } catch (e) {
      if (activeIdRef.current === sessionId) setError(e.message);
    } finally {
      if (activeIdRef.current === sessionId) setBusy(false);
    }
  };

  const answerQuestion = async (choice) => {
    if (!pendingPlan || !pendingQuestion) return;

    if (choice.value === "settings") {
      setTab("settings");
      setSavedMsg("Add a worker URL (and Civitai key) for LoRA installs — OpenRouter still generates without one.");
      return;
    }

    setBusy(true);
    setError("");
    try {
      let plan = pendingPlan;
      if (pendingQuestion.kind === "goal" && choice.id === "example") {
        setPrompt(examplePrompt);
        plan = { ...plan, goal: examplePrompt };
      }

      let installs = [];
      if (pendingQuestion.kind === "lora") {
        if (choice.value === "all") installs = "all";
        else if (choice.value == null) installs = [];
        else installs = [choice.value];
      } else if (pendingQuestion.kind === "config") {
        installs = [];
      } else if ((plan.pendingInstalls || []).length === 1 && canAutoInstall()) {
        installs = plan.pendingInstalls;
      }

      await executePlan(plan, { installs, yolo: false });
      setSavedMsg("Continued after your answer.");
    } catch (e) {
      setError(e.message);
    } finally {
      setBusy(false);
    }
  };

  const dismissPending = () => {
    setPendingPlan(null);
    setPendingQuestion(null);
    setSavedMsg("Dismissed.");
  };

  const persistApiKeys = async (orKey, civKey, { flash = true } = {}) => {
    const wroteOr = Boolean(orKey);
    const wroteCiv = Boolean(civKey);
    if (!wroteOr && !wroteCiv) return null;
    persistStudioTokenLocal();
    const body = {};
    if (wroteOr) body.openrouterApiKey = orKey;
    if (wroteCiv) body.civitaiApiKey = civKey;
    const st = await api("/v1/settings", {
      method: "PUT",
      body: JSON.stringify(body),
    });
    setSettings(st);
    if (wroteOr) setOpenrouterKey("");
    if (wroteCiv) setCivitaiKey("");
    await refresh();
    if (wroteOr) await refreshModels();
    if (flash) {
      const parts = [];
      if (wroteOr) parts.push("OpenRouter");
      if (wroteCiv) parts.push("Civitai");
      const which = `${parts.join(" + ")} key${parts.length > 1 ? "s" : ""} saved`;
      const disk =
        st?.persisted === false
          ? " (warning: secrets file write failed — may not survive API restart)"
          : st?.persisted
            ? " · on API secrets file"
            : "";
      flashSave(
        "settings",
        mock
          ? "Mock API key saved in-memory (reload resets unless you keep this tab open)."
          : `${which}${disk}.`,
      );
    }
    return st;
  };

  const saveSettings = async () => {
    setBusy(true);
    setError("");
    setSavedMsg("");
    try {
      // Persist studio bearer first so the PUT below authenticates when FREYA_API_TOKEN is set.
      if (studioToken.trim()) {
        localStorage.setItem("freya.apiToken", studioToken.trim());
      } else {
        localStorage.removeItem("freya.apiToken");
      }
      const body = { diffusionWorkerUrl: workerUrl };
      if (civitaiKey.trim()) body.civitaiApiKey = civitaiKey.trim();
      if (openrouterKey.trim()) body.openrouterApiKey = openrouterKey.trim();
      const st = await api("/v1/settings", {
        method: "PUT",
        body: JSON.stringify(body),
      });
      setSettings(st);
      applyPrivacyAlignmentFromSettings(st);
      applyInstructionsFromSettings(st, instructionMode);
      markPrivacyAlignmentSynced(prefs);
      // Clear inputs — keys are never echoed; show on-file badges instead of empty "lost" fields.
      setCivitaiKey("");
      setOpenrouterKey("");
      setSavedMsg(
        mock
          ? "Mock settings saved in-memory (reload resets unless you keep this tab open)."
          : "Settings saved. Provider keys stay on the API; studio bearer stays in this browser.",
      );
    } catch (e) {
      setError(e.message);
    } finally {
      setBusy(false);
    }
  };

  const saveInstructionPrompt = async () => {
    if (!instructionMode) return;
    setInstructionBusy(true);
    setError("");
    setSavedMsg("");
    try {
      persistStudioTokenLocal();
      const st = await api("/v1/settings", {
        method: "PUT",
        body: JSON.stringify({
          instructionOverrides: {
            [instructionMode]: instructionDraft,
          },
        }),
      });
      setSettings(st);
      applyInstructionsFromSettings(st, instructionMode);
      const disk =
        st?.persisted === false
          ? " (warning: secrets file write failed)"
          : st?.persisted
            ? " · on API secrets file"
            : "";
      flashSave("settings", `System prompt “${instructionMode}” saved${disk}.`);
    } catch (e) {
      setError(e.message);
    } finally {
      setInstructionBusy(false);
    }
  };

  const resetInstructionPrompt = async () => {
    if (!instructionMode) return;
    setInstructionBusy(true);
    setError("");
    setSavedMsg("");
    try {
      persistStudioTokenLocal();
      const st = await api("/v1/settings", {
        method: "PUT",
        body: JSON.stringify({
          instructionOverrides: {
            [instructionMode]: "",
          },
        }),
      });
      setSettings(st);
      applyInstructionsFromSettings(st, instructionMode);
      flashSave(
        "settings",
        `System prompt “${instructionMode}” reset to pack default.`,
      );
    } catch (e) {
      setError(e.message);
    } finally {
      setInstructionBusy(false);
    }
  };

  const clearApiKey = async (which) => {
    const label = which === "openrouter" ? "OpenRouter" : "Civitai";
    if (
      !window.confirm(
        `Remove the saved ${label} API key from this Freya API? Generation / search that needs it will stop until you paste a new key.`,
      )
    ) {
      return;
    }
    setBusy(true);
    setError("");
    setSavedMsg("");
    setSaveFlash(null);
    cancelPrefsAutosave();
    try {
      const body =
        which === "openrouter"
          ? { openrouterApiKey: "" }
          : { civitaiApiKey: "" };
      const st = await api("/v1/settings", {
        method: "PUT",
        body: JSON.stringify(body),
      });
      setSettings(st);
      if (which === "openrouter") {
        setOpenrouterKey("");
        setOrMetrics(null);
      } else {
        setCivitaiKey("");
      }
      await refresh();
      if (which === "openrouter") await refreshModels();
      flashSave(
        "settings",
        mock
          ? `${label} key cleared (mock).`
          : `${label} key cleared · secrets file updated.`,
      );
    } catch (e) {
      setError(e.message);
    } finally {
      setBusy(false);
    }
  };

  const refreshOpenRouterMetrics = async () => {
    setOrMetricsBusy(true);
    setError("");
    try {
      const data = await api("/v1/metrics/openrouter");
      setOrMetrics(data);
    } catch (e) {
      setOrMetrics(null);
      setError(e.message);
    } finally {
      setOrMetricsBusy(false);
    }
  };

  useEffect(() => {
    if (tab !== "metrics") return undefined;
    if (!mock && !hasBrowserBearer() && authStatus?.authRequired) return undefined;
    refreshOpenRouterMetrics();
    return undefined;
  }, [tab, mock, studioToken, authStatus?.authRequired, settings?.openRouterConfigured]);

  // Debounced auto-save of privacy/alignment → rebuild server catalogs → refresh pickers.
  useEffect(() => {
    if (!prefsReadyRef.current) return undefined;
    const snap = privacyAlignmentSnapshot({
      privacyZdr,
      privacyDataCollection,
      chatProviders,
      modelAlignment,
      allowNsfwLocal,
      instructionTone,
      autoFallbacks,
    });
    const fp = privacyAlignmentFingerprint(snap);
    if (fp === prefsFpRef.current) return undefined;

    if (prefsDebounceRef.current) clearTimeout(prefsDebounceRef.current);
    const gen = ++prefsGenRef.current;
    prefsDebounceRef.current = setTimeout(async () => {
      prefsDebounceRef.current = null;
      try {
        persistStudioTokenLocal();
        const st = await api("/v1/settings", {
          method: "PUT",
          body: JSON.stringify(snap),
        });
        if (gen !== prefsGenRef.current) return;
        setSettings(st);
        markPrivacyAlignmentSynced(snap);
        await refreshModels();
      } catch (e) {
        if (gen === prefsGenRef.current) {
          setError(e.message || "Failed to update model catalog prefs");
        }
      }
    }, PREFS_MODELS_DEBOUNCE_MS);

    return () => {
      if (prefsDebounceRef.current) {
        clearTimeout(prefsDebounceRef.current);
        prefsDebounceRef.current = null;
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- intentional: debounce catalog prefs only
  }, [
    privacyZdr,
    privacyDataCollection,
    chatProviders,
    modelAlignment,
    allowNsfwLocal,
    instructionTone,
    autoFallbacks,
  ]);

  // Debounced auto-save of pasted API keys (empty fields leave on-file secrets alone).
  useEffect(() => {
    const orKey = openrouterKey.trim();
    const civKey = civitaiKey.trim();
    if (!orKey && !civKey) return undefined;

    if (apiKeysDebounceRef.current) clearTimeout(apiKeysDebounceRef.current);
    const gen = ++apiKeysGenRef.current;
    apiKeysDebounceRef.current = setTimeout(async () => {
      apiKeysDebounceRef.current = null;
      try {
        await persistApiKeys(orKey, civKey);
      } catch (e) {
        if (gen === apiKeysGenRef.current) {
          setError(e.message || "Failed to save API key");
        }
      }
    }, API_KEYS_DEBOUNCE_MS);

    return () => {
      if (apiKeysDebounceRef.current) {
        clearTimeout(apiKeysDebounceRef.current);
        apiKeysDebounceRef.current = null;
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- intentional: debounce key paste only
  }, [openrouterKey, civitaiKey]);

  const searchCivitai = async (opts = {}) => {
    const q = String(opts.query ?? civitaiQ).trim();
    const immediate = Boolean(opts.immediate);

    if (!q) {
      const pending = civitaiAbortRef.current;
      civitaiAbortRef.current = null;
      pending?.abort();
      lastCivitaiSearchQRef.current = "";
      setCivitaiItems([]);
      setBusy(false);
      return;
    }
    // Skip redundant debounced searches; Enter/button always re-runs.
    if (!immediate && q === lastCivitaiSearchQRef.current) return;

    civitaiAbortRef.current?.abort();
    const ac = new AbortController();
    civitaiAbortRef.current = ac;

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

  // Search-as-you-type on the Civitai tab (debounced; AbortController cancels in-flight).
  useEffect(() => {
    if (tab !== "civitai" || civitaiSubview !== "search") return;
    const q = civitaiQ.trim();
    if (!q) {
      const pending = civitaiAbortRef.current;
      civitaiAbortRef.current = null;
      pending?.abort();
      lastCivitaiSearchQRef.current = "";
      setCivitaiItems([]);
      setBusy(false);
      return;
    }
    if (q === lastCivitaiSearchQRef.current) return;
    const t = setTimeout(() => {
      searchCivitai({ query: q });
    }, CIVITAI_SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- intentional: debounce on query/tab only
  }, [civitaiQ, tab, civitaiSubview]);

  useEffect(() => {
    if (tab !== "civitai") return;
    // Keep installed + jobs warm so search/favorites/detail can flip Install → Delete / Downloading…
    refreshInstalledAssets();
    refreshDownloadJobs();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab, civitaiSubview]);

  useEffect(() => {
    if (tab !== "worker") return undefined;
    refreshInstalledAssets({ quiet: true });
    return undefined;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab]);

  useEffect(() => {
    if (menuOpen !== "gen-bundle" || generateBackend !== "local") return undefined;
    refreshInstalledAssets({ quiet: true });
    return undefined;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [menuOpen, generateBackend]);

  useEffect(() => {
    const active = downloadJobs.some((j) =>
      ["queued", "downloading"].includes(j.status),
    );
    const onCivitai = tab === "civitai";
    // Follow in-flight installs from any tab. Success must refresh worker lists
    // even when this effect restarts because the job just left "downloading".
    if (!active && !(onCivitai && downloadJobs.length > 0)) return undefined;
    const tick = async () => {
      const jobs = await refreshDownloadJobs();
      if (!Array.isArray(jobs)) return;
      const newlySucceeded = [];
      for (const j of jobs) {
        if (j.status !== "succeeded" || !j.id) continue;
        if (!lastSucceededJobIdsRef.current.has(j.id)) {
          lastSucceededJobIdsRef.current.add(j.id);
          newlySucceeded.push(j.id);
        }
      }
      if (newlySucceeded.length) {
        await refreshInstalledAssets({ quiet: true });
      }
    };
    const ms = active || (onCivitai && civitaiSubview === "downloads") ? 1500 : 5000;
    const t = setInterval(tick, ms);
    const soon = setTimeout(tick, 250);
    return () => {
      clearInterval(t);
      clearTimeout(soon);
    };
    // Poll cadence depends on whether anything is active — not on every jobs array identity.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    tab,
    civitaiSubview,
    downloadJobs.some((j) => ["queued", "downloading"].includes(j.status)),
    downloadJobs.length > 0,
  ]);

  // Drop legacy pinned checkpoints / VAEs so they are never sent as session LoRAs.
  useEffect(() => {
    setSession((prev) => {
      const pinned = prev.pinnedLoras || [];
      const next = pinned.filter(isPinnableAsset);
      if (next.length === pinned.length) return prev;
      return { ...prev, pinnedLoras: next };
    });
  }, [session.pinnedLoras]);

  const installCivitai = async (item) => {
    if (!item.downloadUrl) {
      setError("No download URL on this card");
      return;
    }
    if (!hasWorker()) {
      pinLora(item);
      setSavedMsg(
        `Pinned “${item.name}” for later — Install needs a diffusion worker. Cloud Generate still works via OpenRouter.`,
      );
      return;
    }
    if (!settings?.civitaiConfigured) {
      setError("Add a Civitai API key in Settings to install LoRAs onto your worker.");
      return;
    }
    setBusy(true);
    setError("");
    try {
      const dest = await installOne(item);
      setSavedMsg(`Queued install of ${item.name} → ${dest} on the diffusion worker. Pinned for session.`);
    } catch (e) {
      setError(e.message);
    } finally {
      setBusy(false);
    }
  };

  const gallerySrc = (g) => g.url || `/v1/gallery/${g.id}/image`;

  const leaveMock = () => {
    localStorage.removeItem("freya.mock");
    const u = new URL(window.location.href);
    u.searchParams.delete("mock");
    window.location.href = u.toString();
  };

  const stayInMock = () => {
    localStorage.setItem("freya.mock", "1");
  };

  const clearSuggestions = () => {
    setIdeas([]);
    setSuggestedLoras([]);
    setSuggestNote("");
    setCivitaiQuery("");
  };

  const useStyleExample = () => {
    setPrompt(examplePrompt);
    setTab("gallery");
  };

  const chronologicalTurns = [...(session.turns || [])].reverse();

  // Keep the newest history in view above the pinned composer.
  useEffect(() => {
    const el = timelineRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [
    activeSessionId,
    chronologicalTurns.length,
    sessionLog.length,
    ideas.length,
    pendingPlan,
    pendingQuestion,
  ]);

  const hasTimeline =
    Boolean(session.goal) ||
    chronologicalTurns.length > 0 ||
    (session.pinnedLoras || []).length > 0 ||
    Boolean(session.sessionNote);

  const renderLoraCard = (item, { compact = false, layout = "card", onAfterPin } = {}) => {
    const pinned = isPinned(item);
    const favorited = isFavorite(item);
    const pinnable = isPinnableAsset(item);
    const typeLabel = item.modelType || item.kind || "LORA";
    const downloads = formatStat(item.stats?.downloadCount);
    const thumbUrl = pickCivitaiThumb(item, favorites);
    const cardKey = item.path ? `path:${item.path}` : loraKey(item);
    const row = layout === "row";
    return (
      <article
        key={cardKey}
        role={row ? "row" : undefined}
        className={row ? "lora-card asset-match-row" : `lora-card${compact ? " compact" : ""}`}
      >
        <button
          type="button"
          className="lora-card-thumb lora-card-thumb-btn"
          onClick={() => openCivitaiDetail(item)}
          title="View details"
        >
          {thumbUrl ? (
            <img
              src={thumbUrl}
              alt=""
              loading="lazy"
              onError={(e) => {
                e.currentTarget.classList.add("is-broken");
              }}
            />
          ) : null}
          <div className="lora-card-thumb-fallback" aria-hidden="true">
            {typeLabel.slice(0, 4)}
          </div>
          <span className="lora-type-badge">{typeLabel}</span>
          {downloads ? <span className="lora-stat-badge">{downloads}</span> : null}
        </button>
        <div className="lora-card-body">
          <h4>{item.name}</h4>
          <p>
            {item.modelType || "LORA"}
            {item.triggerWord ? ` · trigger: ${item.triggerWord}` : ""}
          </p>
        </div>
        <div className="row lora-card-actions">
          <button
            type="button"
            className={pinned ? "primary" : "ghost"}
            onClick={() => (pinned ? unpinLora(item) : pinLora(item))}
          >
            {pinned ? "Unpin" : "Pin"}
          </button>
          <button
            type="button"
            className="ghost"
            disabled={busy}
            title={
              hasWorker()
                ? "Install onto your diffusion worker"
                : "Needs a worker URL — pin for later, or add a worker in Settings"
            }
            onClick={() => installCivitai(item)}
          >
            {hasWorker() ? "Install" : "Install (needs worker)"}
          </button>
        </div>
      </article>
    );
  };

  const renderAssetLookup = ({ existing, fresh }) => {
    if (!existing.length && !fresh.length) return null;
    const rows = (items) => items.map((item) => renderLoraCard(item, { layout: "row" }));
    return (
      <div className="asset-match-table" role="table" aria-label="Civitai matches">
        {existing.length > 0 && (
          <div className="asset-match-group" role="rowgroup" aria-label="Installed">
            <div className="asset-match-label">Installed</div>
            {rows(existing)}
          </div>
        )}
        {fresh.length > 0 && (
          <div className="asset-match-group" role="rowgroup" aria-label="Other Civitai matches">
            <div className="asset-match-label">Other Civitai matches</div>
            <div className="asset-match-scroll">
              <div className="asset-match-group">{rows(fresh)}</div>
            </div>
          </div>
        )}
      </div>
    );
  };

  const detailModel = civitaiDetail?.model;
  const detailVersions = detailModel?.modelVersions || [];
  const detailSelectedVersion =
    detailVersions.find((v) => String(v.id) === String(detailModel?.versionId)) ||
    detailVersions[0] ||
    null;
  const detailFiles = detailSelectedVersion?.files || [];
  const detailThumbHint = detailModel
    ? pickCivitaiThumb(detailModel, favorites)
    : null;
  const detailImages =
    (detailModel?.imageUrls?.length
      ? detailModel.imageUrls
      : detailThumbHint
        ? [detailThumbHint]
        : detailModel?.thumbnailUrl
          ? [detailModel.thumbnailUrl]
          : []) || [];
  const detailRelated = civitaiDetail?.related || [];

  const chatLookup = presentLookup(suggestedLoras, installedAssets, civitaiQuery || "");
  const chatLookupCount = chatLookup.existing.length + chatLookup.fresh.length;

  const showAssistPanel =
    ideas.length > 0 || chatLookupCount > 0 || suggestNote || civitaiQuery;

  const modeOptions = AUTOMATION_MODES.map((id) => ({
    id,
    label: AUTOMATION_LABELS[id],
  }));

  return (
    <div className="shell">
      {mock && (
        <div className="mock-banner" role="status">
          <span className="mock-banner-mark">Mock</span>
          <span className="mock-banner-text">
            In-memory atelier — Suggest, pin LoRAs, and Run session without a live API.
          </span>
          <div className="mock-banner-actions">
            <button type="button" className="ghost mock-banner-btn" onClick={stayInMock}>
              Remember
            </button>
            {mockLocked ? (
              <span className="mock-banner-text" title="Started with VITE_FREYA_MOCK=true">
                Dev mock (restart without VITE_FREYA_MOCK to use live API)
              </span>
            ) : (
              <button type="button" className="ghost mock-banner-btn" onClick={leaveMock}>
                Exit
              </button>
            )}
          </div>
        </div>
      )}

      <header className="topbar">
        <div className="brand-lockup">
          <span className="brand-mark" aria-hidden="true">
            <img src="/icon-portrait.png" alt="" width="34" height="34" />
          </span>
          <span className="brand-name">
            Freya<span className="brand-dot">.</span>
          </span>
          <span className="brand-sub">agentic image studio</span>
        </div>
        <div className="topbar-status">
          <button
            type="button"
            className="theme-toggle"
            onClick={toggleTheme}
            title={theme === "dark" ? "Light" : "Dark"}
            aria-label={theme === "dark" ? "Switch to light theme" : "Switch to dark theme"}
            aria-pressed={theme === "light"}
          >
            {theme === "dark" ? <IconSun /> : <IconMoon />}
            <span className="sr-only">{theme === "dark" ? "Light" : "Dark"}</span>
          </button>
          {mock && <span className="topbar-mock">mock</span>}
          {settings && (
            <span className="tabs-status" title="Worker is optional — needed for LoRA installs and local/custom generation">
              <span className={`status-dot ${settings.openRouterConfigured ? "on" : ""}`} />
              OpenRouter
              <span className={`status-dot ${settings.civitaiConfigured ? "on" : ""}`} />
              Civitai
              <button
                type="button"
                className="tabs-status-worker"
                onClick={() => setTab("worker")}
                title="Open Worker tab"
              >
                <span
                  className={`status-dot ${
                    workerStats?.reachable
                      ? "on"
                      : settings.diffusionWorkerUrl
                        ? "warn"
                        : ""
                  }`}
                />
                Worker
              </button>
            </span>
          )}
          <span className="topbar-mode">{AUTOMATION_LABELS[automationMode]}</span>
        </div>
      </header>

      <div className={`studio${sidebarOpen ? " sidebar-open" : " sidebar-closed"}`}>
        <aside
          id="freya-sessions"
          className="sessions-sidebar"
          aria-label="Session history"
          hidden={!sidebarOpen}
        >
          <div className="sessions-sidebar-head">
            <button type="button" className="primary sessions-new" onClick={startNewSession}>
              Start new session
            </button>
          </div>
          <ul className="session-list" role="listbox" aria-label="Sessions">
            {sessions.some((s) => !s.archived) ? (
              sessions
                .filter((s) => !s.archived)
                .map((s) => {
                  const active = s.id === activeSessionId;
                  const title = active
                    ? sessionTitleFrom(prompt, session)
                    : s.title || sessionTitleFrom(s.prompt, s.session);
                  return (
                    <SessionRow
                      key={s.id}
                      title={title}
                      updatedAt={s.updatedAt}
                      createdAt={s.createdAt}
                      imageCount={sessionImageCount(active ? { ...s, session, results: batchResults, result } : s)}
                      active={active}
                      confirming={confirmDeleteId === s.id}
                      onOpen={() => switchSession(s.id)}
                      onArchive={() => setSessionArchived(s.id, true)}
                      onAskDelete={() => setConfirmDeleteId(s.id)}
                      onConfirmDelete={() => deleteSession(s.id)}
                      onCancelDelete={() => setConfirmDeleteId((cur) => (cur === s.id ? null : cur))}
                    />
                  );
                })
            ) : (
              <li className="session-list-empty" role="presentation">No active sessions</li>
            )}
          </ul>
          {sessions.some((s) => s.archived) && (
            <div className="session-archived">
              <button
                type="button"
                className="session-archived-toggle"
                aria-expanded={archivedOpen}
                onClick={() => setArchivedOpen((open) => !open)}
              >
                <span>Archived</span>
                <span className="session-archived-count">
                  {sessions.filter((s) => s.archived).length}
                </span>
              </button>
              {archivedOpen && (
                <ul className="session-list" role="listbox" aria-label="Archived sessions">
                  {sessions
                    .filter((s) => s.archived)
                    .map((s) => {
                      const active = s.id === activeSessionId;
                      const title = active
                        ? sessionTitleFrom(prompt, session)
                        : s.title || sessionTitleFrom(s.prompt, s.session);
                      return (
                        <SessionRow
                          key={s.id}
                          title={title}
                          updatedAt={s.updatedAt}
                          createdAt={s.createdAt}
                          imageCount={sessionImageCount(
                            active ? { ...s, session, results: batchResults, result } : s,
                          )}
                          active={active}
                          archived
                          confirming={confirmDeleteId === s.id}
                          onOpen={() => switchSession(s.id)}
                          onUnarchive={() => setSessionArchived(s.id, false)}
                          onAskDelete={() => setConfirmDeleteId(s.id)}
                          onConfirmDelete={() => deleteSession(s.id)}
                          onCancelDelete={() =>
                            setConfirmDeleteId((cur) => (cur === s.id ? null : cur))
                          }
                        />
                      );
                    })}
                </ul>
              )}
            </div>
          )}
        </aside>

      <div className="workspace">
        <section className="pane pane-left" aria-label="Prompt and session">
          <div className="timeline" aria-label="Session timeline" ref={timelineRef}>
            <div className="timeline-head">
              <h2>Session</h2>
              <span className="timeline-count">
                {chronologicalTurns.length
                  ? `${chronologicalTurns.length} turn${chronologicalTurns.length === 1 ? "" : "s"}`
                  : "no turns yet"}
              </span>
              <button
                type="button"
                className="ghost session-clear"
                onClick={() => {
                  setSession(emptySession());
                  setPendingPlan(null);
                  setPendingQuestion(null);
                  setSessionLog([]);
                  clearSuggestions();
                }}
              >
                Clear
              </button>
            </div>

            {!hasTimeline && !pendingPlan && !pendingQuestion && sessionLog.length === 0 && (
              <p className="timeline-empty">
                Your goals, plans, questions and turns will collect here. Try the featured example,
                then Suggest ideas or Run session.
              </p>
            )}

            {session.goal && (
              <article className="t-entry t-goal">
                <span className="t-kind">Goal</span>
                <p>{session.goal}</p>
              </article>
            )}

            {(session.pinnedLoras || []).length > 0 && (
              <article className="t-entry t-pins">
                <span className="t-kind">Pinned LoRAs</span>
                <ul className="pin-list">
                  {(session.pinnedLoras || []).map((l) => (
                    <li key={loraKey(l)}>
                      <span>
                        {l.name}
                        {l.triggerWord ? ` · ${l.triggerWord}` : ""}
                      </span>
                      <button type="button" className="ghost" onClick={() => unpinLora(l)}>
                        Unpin
                      </button>
                    </li>
                  ))}
                </ul>
              </article>
            )}

            {chronologicalTurns.map((t, i) => (
              <article key={`${t.at}-${i}`} className="t-entry t-turn">
                <div className="t-meta">
                  <span className="t-kind">Turn {i + 1}</span>
                  {t.at && (
                    <time>
                      {new Date(t.at).toLocaleTimeString([], {
                        hour: "numeric",
                        minute: "2-digit",
                      })}
                    </time>
                  )}
                  {t.artifactIds?.length > 0 && (
                    <span>
                      {t.artifactIds.length} image{t.artifactIds.length === 1 ? "" : "s"} →
                    </span>
                  )}
                </div>
                <p className="t-bubble">{m.content}</p>
                {m.role !== "user" &&
                  (() => {
                    const offer = generateOfferForTurn(conversation, i);
                    if (!offer) return null;
                    return (
                      <div className="t-generate-row">
                        <button
                          type="button"
                          className="primary t-generate"
                          disabled={busy}
                          onClick={() => runGenerateOffer(offer)}
                        >
                          Generate
                        </button>
                      </div>
                    );
                  })()}
                {Array.isArray(m.actions) && m.actions.length > 0 && (
                  <ul className="t-tools">
                    {m.actions.map((a, ai) => (
                      <li key={`${a.type}-${ai}`} className={`t-tool t-tool-${a.status || "done"}`}>
                        <span className="t-tool-type">{a.type}</span>
                        <span className="t-tool-status">{a.status || "done"}</span>
                        {a.status === "error" && a.error ? ` — ${a.error}` : ""}
                      </li>
                    ))}
                  </ul>
                )}
                {m.artifactIds?.length > 0 && (
                  <div className="t-thumbs">
                    {m.artifactIds.map((id) => {
                      const art =
                        batchResults.find((r) => r.artifactId === id) ||
                        (result?.artifactId === id ? result : null) ||
                        gallery.find((g) => g.id === id);
                      const src = art?.url || (art ? gallerySrc(art) : `/v1/gallery/${id}/image`);
                      const file = art || { id, artifactId: id };
                      return (
                        <div className="t-thumb-wrap" key={id}>
                          <button
                            type="button"
                            className="t-thumb"
                            onClick={() => {
                              if (art) selectGalleryPreview(art);
                            }}
                          >
                            {isVideoArtifact(art) ? (
                              <video src={src} muted playsInline preload="metadata" />
                            ) : (
                              <img src={src} alt="" />
                            )}
                          </button>
                          <MediaDownload
                            src={src}
                            item={file}
                            className="ghost icon-btn t-thumb-dl"
                            onError={setError}
                          />
                        </div>
                      );
                    })}
                  </div>
                )}
                {Array.isArray(m.loras) && m.loras.length > 0 && (
                  <p className="civitai-search-line">
                    Found {m.loras.length} Civitai match{m.loras.length === 1 ? "" : "es"}
                    {m.civitaiQuery ? (
                      <>
                        {" "}
                        for <strong>{m.civitaiQuery}</strong>
                      </>
                    ) : null}
                    . Browse them on{" "}
                    <button type="button" className="linkish" onClick={() => setTab("styles")}>
                      Styles
                    </button>
                    .
                  </p>
                )}
                {Array.isArray(m.ideas) && m.ideas.length > 0 && (
                  <div className="idea-chips">
                    {m.ideas.map((idea) => (
                      <button
                        key={idea}
                        type="button"
                        className="idea-chip"
                        onClick={() => setPrompt(idea)}
                      >
                        {idea}
                      </button>
                    ))}
                  </div>
                )}
              </article>
            ))}

            {session.sessionNote && (
              <article className="t-entry t-note">
                <span className="t-kind">Session note</span>
                <p>{session.sessionNote}</p>
              </article>
            )}

            {pendingPlan && pendingPlan.status === "plan" && (
              <aside className="pending-plan" aria-label="Session plan">
                <div className="pending-plan-head">
                  <h3>Session plan</h3>
                  <button type="button" className="ghost" onClick={dismissPending}>
                    Dismiss
                  </button>
                </div>
                <p className="pending-goal">
                  <span className="session-label">Goal</span>
                  {pendingPlan.goal}
                </p>
                {pendingPlan.civitaiQuery && (
                  <p className="civitai-search-line" role="status">
                    Proposed Civitai search: <strong>{pendingPlan.civitaiQuery}</strong>
                  </p>
                )}
                {pendingPlan.steps?.length > 0 && (
                  <ol className="pending-steps">
                    {pendingPlan.steps.map((s, i) => (
                      <li key={`${s.kind}-${i}`}>{s.note || s.kind}</li>
                    ))}
                  </ol>
                )}
                {pendingPlan.pendingInstalls?.length > 0 && (
                  <div className="pending-installs">
                    <span className="session-label">LoRA candidates</span>
                    <ul>
                      {pendingPlan.pendingInstalls.map((l) => (
                        <li key={loraKey(l)}>
                          “{l.name}”
                          <button
                            type="button"
                            className="ghost"
                            disabled={busy}
                            onClick={() => installCivitai(l)}
                          >
                            Install
                          </button>
                        </li>
                      ))}
                    </ul>
                    <p className="hint">
                      Plan mode has no side effects until you Install or Generate yourself — or
                      switch to Auto/YOLO. Install needs a worker; Generate via OpenRouter does not.
                    </p>
                  </div>
                )}
                {pendingPlan.batchPrompts?.length > 0 && (
                  <div className="pending-batch">
                    <span className="session-label">Batch prompts</span>
                    <ul>
                      {pendingPlan.batchPrompts.map((bp) => (
                        <li key={bp}>
                          <button type="button" className="linkish" onClick={() => setPrompt(bp)}>
                            {bp.slice(0, 100)}
                            {bp.length > 100 ? "…" : ""}
                          </button>
                        </li>
                      ))}
                    </ul>
                  </div>
                )}
              </aside>
            )}

            {pendingQuestion && (
              <aside className="pending-plan pending-ask" aria-label="Freya question">
                <div className="pending-plan-head">
                  <h3>Freya needs a decision</h3>
                  <button type="button" className="ghost" onClick={dismissPending}>
                    Dismiss
                  </button>
                </div>
                <p className="pending-ask-text">{pendingQuestion.text}</p>
                {pendingPlan?.civitaiQuery && (
                  <p className="civitai-search-line" role="status">
                    Civitai search: <strong>{pendingPlan.civitaiQuery}</strong>
                  </p>
                )}
                {pendingPlan?.batchPrompts?.length > 0 && (
                  <p className="hint">
                    Queued: {pendingPlan.batchPrompts.length} variations
                    {pendingPlan.pendingInstalls?.length
                      ? ` · ${pendingPlan.pendingInstalls.length} LoRA candidate(s)`
                      : ""}
                  </p>
                )}
                <div className="pending-ask-choices">
                  {pendingQuestion.choices.map((c) => (
                    <button
                      key={c.id}
                      type="button"
                      className={c.id === "settings" ? "ghost" : "primary"}
                      disabled={busy}
                      onClick={() => answerQuestion(c)}
                    >
                      {c.label}
                    </button>
                  ))}
                </div>
              </aside>
            )}

            {sessionLog.length > 0 && (
              <aside className="session-log" aria-label="Session activity">
                <span className="session-label">What Freya did</span>
                <ul>
                  {sessionLog.map((line, i) => (
                    <li key={`${i}-${line.slice(0, 24)}`}>{line}</li>
                  ))}
                </ul>
              </aside>
            )}

            {showAssistPanel && (
              <aside className="suggestions">
                <div className="suggestions-head">
                  <h3>Ideas from Freya</h3>
                  <button type="button" className="ghost" onClick={clearSuggestions}>
                    Dismiss
                  </button>
                </div>
                {civitaiQuery && (
                  <p className="civitai-search-line" role="status">
                    Searching Civitai for: <strong>{civitaiQuery}</strong>
                  </p>
                )}
                {suggestNote && <p className="hint">{suggestNote}</p>}
                {ideas.length > 0 && (
                  <div className="idea-chips">
                    {ideas.map((idea) => (
                      <button
                        key={idea}
                        type="button"
                        className="idea-chip"
                        onClick={() => {
                          setPrompt(idea);
                          setSession((prev) => ({
                            ...prev,
                            goal: prev.goal || idea,
                          }));
                        }}
                      >
                        {idea}
                      </button>
                    ))}
                  </div>
                )}
                {chatLookupCount > 0 && (
                  <p className="hint">
                    {chatLookupCount} match{chatLookupCount === 1 ? "" : "es"} on{" "}
                    <button type="button" className="linkish" onClick={() => setTab("styles")}>
                      Styles
                    </button>
                    . Installed rows match this lookup by name, filename, or version id.
                  </p>
                )}
              </aside>
            )}
          </div>

          <div className="compose">
            <div className="compose-head">
              <label className="compose-label" htmlFor="freya-prompt">
                What are you going for?
              </label>
              {!prompt.trim() && (
                <button
                  type="button"
                  className="linkish compose-demo-btn"
                  onClick={useStyleExample}
                  title={`Use the featured example — Freya may search Civitai for “${exampleCivitaiQuery}”`}
                >
                  Use example
                </button>
              )}
            </div>
            <textarea
              id="freya-prompt"
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder={examplePrompt}
              rows={3}
              title="Everyday language or classic SD prompts (tags, weights, triggers) — one box, Freya handles either"
              aria-describedby="freya-prompt-hint"
            />
            <p id="freya-prompt-hint" className="sr-only">
              Everyday language or classic SD prompts (tags, weights, triggers) — one box, Freya
              handles either. Cloud Generate works with OpenRouter alone; add a worker for LoRAs and
              custom local looks. {MODE_HINTS[automationMode]}
            </p>
            {error && <p className="err compose-status">{error}</p>}
            {savedMsg && (
              <p className="hint saved-msg compose-status" role="status">
                {savedMsg}
              </p>
            )}
            <div className="composer-bar" aria-label="Session controls">
              <div className="compose-actions">
                <button
                  type="button"
                  className={`ghost icon-btn sidebar-toggle${sidebarOpen ? " open" : ""}`}
                  aria-pressed={sidebarOpen}
                  aria-controls="freya-sessions"
                  title={sidebarOpen ? "Hide sessions" : "Show sessions"}
                  aria-label={sidebarOpen ? "Hide sessions" : "Show sessions"}
                  onClick={() => setSidebar(!sidebarOpen)}
                >
                  <IconSessions />
                  <span className="sr-only">
                    {sidebarOpen ? "Hide sessions" : "Show sessions"}
                  </span>
                </button>
              </div>
              <div className="chrome-strip" role="toolbar" aria-label="Generation backend">
                <div
                  className="backend-switch"
                  role="group"
                  aria-label="Generation backend"
                >
                  <button
                    type="button"
                    className={`backend-switch-btn${generateBackend === "cloud" ? " is-active" : ""}`}
                    aria-pressed={generateBackend === "cloud"}
                    onClick={() => changeGenerateBackend("cloud")}
                    title="OpenRouter cloud generate"
                  >
                    Cloud
                  </button>
                  <button
                    type="button"
                    className={`backend-switch-btn${generateBackend === "local" ? " is-active" : ""}`}
                    aria-pressed={generateBackend === "local"}
                    disabled={!localBackendOk}
                    onClick={() => changeGenerateBackend("local")}
                    title={
                      localBackendOk
                        ? "Local GPU worker generate"
                        : localBackendReason || "Local worker unavailable"
                    }
                  >
                    Local
                  </button>
                </div>
              </div>
              <div
                className="composer-primary-cluster"
                role="group"
                aria-label="Models, cost, suggest, send, and run session"
              >
                <GroupedChromeMenu
                  className="composer-model-menu"
                  label="Agent, router & mode"
                  icon={<ModeIcon />}
                  align="end"
                  valueLabel={`${modelShortLabel(agentOptions, agentMenuValue)} · ${AUTOMATION_LABELS[automationMode]}`}
                  title={[
                    modelsCatalogReady
                      ? `Agent: ${modelLabel(agentOptions, agentModel)}`
                      : `Agent: Auto — ${MODELS_API_HINT}`,
                    modelsCatalogReady
                      ? `Router: ${modelLabel(routerOptions, routerModel)}`
                      : `Router: Auto — ${MODELS_API_HINT}`,
                    `Mode: ${AUTOMATION_LABELS[automationMode]} — ${MODE_HINTS[automationMode]}`,
                  ].join(" · ")}
                  open={menuOpen === "agent-bundle"}
                  onOpen={(v) => setMenuOpen(v ? "agent-bundle" : null)}
                  sections={[
                    {
                      id: "agent",
                      label: "Agent",
                      icon: <IconAgent />,
                      valueLabel: modelShortLabel(agentOptions, agentMenuValue),
                      options: agentOptions,
                      value: agentMenuValue,
                      onChange: changeAgentModel,
                      menuNote: modelsMenuNote,
                    },
                    {
                      id: "router",
                      label: "Router",
                      icon: <IconRouter />,
                      valueLabel: modelShortLabel(routerOptions, routerMenuValue),
                      options: routerOptions,
                      value: routerMenuValue,
                      onChange: changeRouterModel,
                      menuNote: modelsMenuNote,
                    },
                    {
                      id: "mode",
                      label: "Mode",
                      icon: <ModeIcon />,
                      valueLabel: AUTOMATION_LABELS[automationMode],
                      options: modeOptions,
                      value: automationMode,
                      onChange: setMode,
                    },
                  ]}
                />
                {generateBackend === "cloud" ? (
                  <GroupedChromeMenu
                    className="composer-model-menu"
                    label="Generation models"
                    icon={<IconImage />}
                    align="end"
                    valueLabel={`${modelShortLabel(imageOptions, imageMenuValue)} · ${modelShortLabel(videoOptions, videoMenuValue)}`}
                    title={[
                      modelsCatalogReady
                        ? `Image: ${modelLabel(imageOptions, imageModel)}`
                        : `Image: Auto — ${MODELS_API_HINT}`,
                      modelsCatalogReady
                        ? `Video: ${modelLabel(videoOptions, videoModel)}`
                        : `Video: Auto — ${MODELS_API_HINT}`,
                    ].join(" · ")}
                    open={menuOpen === "gen-bundle"}
                    onOpen={(v) => setMenuOpen(v ? "gen-bundle" : null)}
                    sections={[
                      {
                        id: "image",
                        label: "Image",
                        icon: <IconImage />,
                        valueLabel: modelShortLabel(imageOptions, imageMenuValue),
                        options: imageOptions,
                        value: imageMenuValue,
                        onChange: changeImageModel,
                        menuNote: modelsMenuNote,
                      },
                      {
                        id: "video",
                        label: "Video",
                        icon: <IconVideo />,
                        valueLabel: modelShortLabel(videoOptions, videoMenuValue),
                        options: videoOptions,
                        value: videoMenuValue,
                        onChange: changeVideoModel,
                        menuNote:
                          modelsCatalogReady && videoOptions.length <= 1
                            ? "No T2V models in catalog — OpenRouter video key/plan may be required"
                            : modelsMenuNote,
                      },
                    ]}
                  />
                ) : (
                  <GroupedChromeMenu
                    className="composer-model-menu"
                    label="Local base"
                    icon={<IconImage />}
                    align="end"
                    valueLabel={
                      localBaseModel
                        ? localBaseModel.split("/").pop() || localBaseModel
                        : "Base"
                    }
                    title={
                      localBaseModel
                        ? `Active base: ${localBaseModel}`
                        : "Select worker checkpoint / active base"
                    }
                    open={menuOpen === "gen-bundle"}
                    onOpen={(v) => setMenuOpen(v ? "gen-bundle" : null)}
                    sections={[
                      {
                        id: "base",
                        label: "Base",
                        icon: <IconImage />,
                        valueLabel: localBaseModel
                          ? localBaseModel.split("/").pop() || localBaseModel
                          : "Base",
                        options: localCheckpoints.length
                          ? localCheckpoints.map((c) => ({
                              id: c.path,
                              label: `${c.name || c.id}${c.baseModel ? ` (${c.baseModel})` : ""}`,
                            }))
                          : [
                              {
                                id: "",
                                label: "No checkpoints — install from Civitai",
                                disabled: true,
                              },
                            ],
                        value: localBaseModel,
                        onChange: (id) => {
                          if (id) setComposerBaseModel(id);
                        },
                        menuNote: localBaseBusy
                          ? "Setting active base…"
                          : localBackendReason ||
                            (localCheckpoints.length
                              ? "Worker checkpoint for local generate"
                              : "Install a checkpoint on the worker first"),
                      },
                    ]}
                  />
                )}
                <CostTierMenu
                  value={autoFallbacks.costTier || "medium"}
                  open={menuOpen === "cost"}
                  onOpen={(v) => setMenuOpen(v ? "cost" : null)}
                  onChange={(costTier) =>
                    setAutoFallbacks((prev) =>
                      normalizeAutoFallbacks({ ...prev, costTier }),
                    )
                  }
                />
                <button
                  type="button"
                  className="ghost icon-btn"
                  disabled={busy || (!prompt.trim() && !session.goal)}
                  onClick={runSession}
                  title={`Run session (${AUTOMATION_LABELS[automationMode]})`}
                  aria-label="Run session"
                >
                  <IconLayers />
                  <span className="sr-only">Run session</span>
                </button>
              </div>
              <span className="composer-divider" aria-hidden="true" />
              <div className="chrome-strip" role="toolbar" aria-label="Mode and models">
                <CompactMenu
                  label="Mode"
                  icon={<IconSliders />}
                  title={`Mode: ${AUTOMATION_LABELS[automationMode]} — ${MODE_HINTS[automationMode]}`}
                  valueLabel={AUTOMATION_LABELS[automationMode]}
                  options={modeOptions}
                  value={automationMode}
                  onChange={setMode}
                  open={menuOpen === "mode"}
                  onOpen={(v) => setMenuOpen(v ? "mode" : null)}
                />
                <CompactMenu
                  label="Agent"
                  icon={<IconAgent />}
                  title={`Agent model: ${modelLabel(AGENT_MODELS, agentModel)}`}
                  valueLabel={modelShortLabel(AGENT_MODELS, agentModel)}
                  options={AGENT_MODELS}
                  value={agentModel}
                  onChange={changeAgentModel}
                  open={menuOpen === "agent"}
                  onOpen={(v) => setMenuOpen(v ? "agent" : null)}
                />
                <CompactMenu
                  label="Image"
                  icon={<IconImage />}
                  title={`Image model: ${modelLabel(IMAGE_MODELS, imageModel)}`}
                  valueLabel={modelShortLabel(IMAGE_MODELS, imageModel)}
                  options={IMAGE_MODELS}
                  value={imageModel}
                  onChange={changeImageModel}
                  open={menuOpen === "image"}
                  onOpen={(v) => setMenuOpen(v ? "image" : null)}
                />
              </div>
              <div className="composer-primary-cluster" role="group" aria-label="Suggest and generate">
                <button
                  type="button"
                  className="ghost icon-btn"
                  disabled={busy}
                  onClick={suggestIdeas}
                  title="Suggest ideas"
                  aria-label="Suggest ideas"
                >
                  <IconBulb />
                  <span className="sr-only">Suggest ideas</span>
                </button>
                <button
                  type="button"
                  className="primary icon-btn icon-btn-primary composer-generate"
                  disabled={busy || !prompt.trim()}
                  onClick={run}
                  title={busy ? "Working…" : "Generate"}
                  aria-label={busy ? "Working" : "Generate"}
                >
                  {busy ? <IconSpinner /> : <IconPlay />}
                  <span className="sr-only">{busy ? "Working…" : "Generate"}</span>
                </button>
              </div>
            </div>
          </div>
        </section>

        <section className="pane pane-right" aria-label="Gallery and results">
      {tab === "gallery" && !previewResult && gallery.length === 0 && batchResults.length === 0 && (
        <div className="gallery-empty">
          <div className="gallery-empty-art">
            <img
              src="/empty-gallery.png"
              alt="no images yet"
              className="gallery-empty-img"
              draggable={false}
            />
          </div>
          <figure className="salon-feature">
            <img src={gallerySrc(gallery[0])} alt={gallery[0].prompt || "Latest gallery image"} />
            <figcaption>
              <span className="salon-caption">{gallery[0].prompt}</span>
              <span className="salon-meta">
                {gallery[0].backend}
                {gallery[0].model ? ` · ${gallery[0].model}` : ""}
                {mock ? " · mock" : ""}
              </span>
              <button
                type="button"
                className="ghost remix-btn"
                onClick={async () => {
                  const remix = await api(`/v1/gallery/${gallery[0].id}/remix`, {
                    method: "POST",
                  });
                  setPrompt(remix.prompt || "");
                }}
              >
                Remix this prompt
              </button>
            </figcaption>
          </figure>
        </section>
      )}

      {tab === "gallery" && !result && gallery.length === 0 && batchResults.length === 0 && (
        <div className="gallery-empty">
          <div className="gallery-empty-art" aria-hidden="true">
            <HeroCanvas />
          </div>
          <p>No images yet — Generate or Run session and results will hang here.</p>
        </div>
      )}

      {tab === "gallery" && batchResults.length > 1 && (
        <section className="salon">
          <div className="salon-head">
            <span className="t-kind">Session set</span>
            <p className="salon-sub">Select a work to inspect it above / remix its prompt.</p>
          </div>
          <div className="salon-wall">
            {batchResults.map((r, i) => (
              <article
                key={r.artifactId || r.url}
                className={`salon-work${result?.artifactId === r.artifactId ? " selected" : ""} salon-work-${(i % 3) + 1}`}
                onClick={() => {
                  setResult(r);
                  setModelBrief(r.enhancedPrompt || "");
                  if (r.prompt) setPrompt(r.prompt);
                }}
              >
                {r.url ? (
                  <MediaDownload
                    src={r.url}
                    item={r}
                    className="ghost icon-btn salon-work-dl"
                    onError={setError}
                  />
                ) : null}
                {r.artifactId ? (
                  <button
                    type="button"
                    className="ghost salon-work-del"
                    title="Delete from gallery"
                    aria-label="Delete variation"
                    onClick={(e) => {
                      e.stopPropagation();
                      deleteGalleryItem(r.artifactId);
                    }}
                  >
                    <IconTrash />
                  </button>
                ) : null}
                <figure>
                  <img src={r.url} alt={r.prompt || "variation"} />
                  <figcaption>
                    <span className="salon-caption">
                      {(r.prompt || "").slice(0, 100)}
                      {(r.prompt || "").length > 100 ? "…" : ""}
                    </span>
                    <span className="salon-meta">
                      {r.backend}
                      {r.model ? ` · ${r.model}` : ""}
                      {mock ? " · mock" : ""}
                    </span>
                  </figcaption>
                </figure>
              </article>
            ))}
          </div>
        </section>
      )}

      {tab === "gallery" && result && (
        <section className="salon salon-single">
          <div className="salon-head">
            <span className="t-kind">{batchResults.length > 1 ? "Selected variation" : "Latest image"}</span>
          </div>
          <figure className="salon-feature">
            <img src={result.url} alt={result.enhancedPrompt || prompt} />
            <figcaption>
              <span className="salon-meta">
                {result.backend}
                {result.model ? ` · ${result.model}` : ""}
                {mock ? " · mock" : ""}
              </span>
              {previewResult.artifactId && (
                <div className="salon-actions">
                  <MediaDownload
                    src={previewResult.url}
                    item={previewResult}
                    className="ghost icon-btn salon-dl"
                    onError={setError}
                  />
                  <button
                    type="button"
                    className="ghost remix-btn"
                    onClick={async () => {
                      const remix = await api(
                        `/v1/gallery/${previewResult.artifactId}/remix`,
                        { method: "POST" },
                      );
                      setPrompt(remix.prompt || "");
                    }}
                  >
                    Remix this prompt
                  </button>
                  <button
                    type="button"
                    className="ghost remix-btn salon-del-btn"
                    title="Delete from gallery"
                    onClick={() => deleteGalleryItem(previewResult.artifactId)}
                  >
                    <IconTrash />
                    Delete
                  </button>
                </div>
              )}
            </figcaption>
          </figure>
          {modelBrief && (
            <details className="model-brief">
              <summary>What Freya sent to the model</summary>
              <p>{modelBrief}</p>
            </details>
          )}
        </section>
      )}

      <nav className="tabs" aria-label="Studio sections">
        {["gallery", "styles", "civitai", "settings"].map((t) => (
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
        <section className="panel style-rail">
          {chatLookupCount > 0 && (
            <div className="style-matches">
              <div className="style-matches-head">
                <div>
                  <span className="t-kind">Civitai matches</span>
                  {civitaiQuery ? (
                    <p className="style-matches-q">
                      Civitai · <strong>{civitaiQuery}</strong>
                    </p>
                  ) : (
                    <p className="style-matches-q">Suggested for this prompt</p>
                  )}
                </div>
                <button type="button" className="ghost" onClick={clearSuggestions}>
                  Dismiss
                </button>
              </div>
              <p className="hint">
                Installed rows match this lookup by name, filename, or version id. Other Civitai
                hits stay in the list below. Nothing is downloaded until you choose.
              </p>
              {renderAssetLookup(chatLookup)}
            </div>
          )}
          {styles.map((s) => (
            <article
              key={s.id}
              className="style-item"
              onClick={() => {
                setPrompt(s.examplePrompt);
                setSession((prev) => ({
                  ...prev,
                  goal: prev.goal || s.examplePrompt,
                }));
                setTab("gallery");
              }}
            >
              <h3>{s.displayName}</h3>
              <p>{(s.examplePrompt || "").slice(0, 120)}…</p>
              <span className="salon-meta">{s.backend}</span>
            </article>
          ))}
        </section>
      )}

      {tab === "civitai" && (
        <section className="panel">
          {!settings?.civitaiConfigured && (
            <p className="hint">
              Add your Civitai API key in Settings to search LoRAs
              {mock ? " (any non-empty value works in mock)" : ""}. Install onto a worker is
              optional — Pin works without one; cloud Generate uses OpenRouter.
            </p>
          )}
          {settings?.civitaiConfigured && !hasWorker() && (
            <p className="hint">
              Worker not set — you can search and pin LoRAs now. Install needs a diffusion worker
              URL; Generate still works via OpenRouter without one.
            </p>
          )}
          <p className="hint civitai-demo-hint">
            Featured example: describe something like “{examplePrompt}” — Freya searches for “
            {exampleCivitaiQuery}”. Manual Search/Pin here always works; Install needs a worker.
          </p>
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
          <div className="lora-row">
            {civitaiItems.map((item) => (
              <article key={item.id} className="lora-card">
                <h4>{item.name}</h4>
                <p>{item.modelType}</p>
                {item.triggerWord && (
                  <p className="hint">Trigger (optional): {item.triggerWord}</p>
                )}
                <div className="row lora-card-actions">
                  <button
                    className={isPinned(item) ? "primary" : "ghost"}
                    onClick={() => {
                      if (isPinned(item)) unpinLora(item);
                      else pinLora(item);
                      setTab("gallery");
                    }}
                  >
                    {isPinned(item) ? "Unpin" : "Pin to session"}
                  </button>
                  <button
                    className="primary"
                    disabled={busy}
                    title={
                      hasWorker()
                        ? "Install this LoRA onto your diffusion worker"
                        : "Needs a worker URL — pin for later, or add a worker in Settings"
                    }
                    onClick={() => installCivitai(item)}
                  >
                    {hasWorker() ? "Install to worker" : "Install (needs worker)"}
                  </button>
                </div>
              </article>
            ))}
          </div>
        </section>
      )}

      {tab === "gallery" && (
        <section className="panel salon">
          <div className="salon-wall salon-wall-gallery">
            {gallery.map((g) => (
              <article
                key={g.id}
                className="salon-work"
                onClick={async () => {
                  const remix = await api(`/v1/gallery/${g.id}/remix`, { method: "POST" });
                  setPrompt(remix.prompt || "");
                  setTab("gallery");
                }}
              >
                <MediaDownload
                  src={gallerySrc(g)}
                  item={g}
                  className="ghost icon-btn salon-work-dl"
                  onError={setError}
                />
                <button
                  type="button"
                  className="ghost salon-work-del"
                  title="Delete from gallery"
                  aria-label={`Delete ${(g.prompt || "generation").slice(0, 40)}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    deleteGalleryItem(g.id);
                  }}
                >
                  <IconTrash />
                </button>
                <figure>
                  <img src={gallerySrc(g)} alt={g.prompt} />
                  <figcaption>
                    <span className="salon-caption">{(g.prompt || "").slice(0, 90)}…</span>
                    <span className="salon-meta">{g.backend}</span>
                  </figcaption>
                </figure>
              </article>
            ))}
          </div>
        </section>
      )}

      {tab === "worker" && (
        <section className="panel settings worker-tab" aria-label="Diffusion worker">
          <div className="worker-layout">
            <aside className="worker-setup-panel" aria-label="Setup and connect">
              <div className="settings-block-head">
                <span className="label-with-help">
                  <strong>Setup</strong>
                  <HelpQ topic="help-worker" label="Worker setup" onOpen={openHelp} />
                </span>
              </div>
              <p className="hint">
                Local LoRAs / Local generate need a GPU worker on a host (
                <code>freya-local-worker</code>, default <code>0.0.0.0:{DEFAULT_WORKER_PORT}</code>
                ). Cloud Generate uses OpenRouter. Models dir: <code>FREYA_MODELS_DIR</code>{" "}
                (default <code>~/freya/models</code>).
              </p>

              <div className="worker-setup" aria-label="One-line host setup">
                <strong className="worker-connect-title">One-line host setup</strong>
                <p className="hint">
                  Copy a command onto the GPU machine. It installs/uses Docker, writes{" "}
                  <code>FREYA_WORKER_TOKEN</code> from your machine token, and starts{" "}
                  <code>freya-local-worker</code>. Generate a token first if the field below shows
                  a placeholder.
                </p>
                {!effectiveMachineToken() ? (
                  <p className="hint">
                    No machine token in this browser — use{" "}
                    <strong>Generate machine token</strong> (right), then copy again.
                  </p>
                ) : null}
                <label>
                  Linux / macOS
                  <textarea
                    className="worker-setup-cmd"
                    readOnly
                    rows={3}
                    value={linuxWorkerSetupCmd()}
                    spellCheck={false}
                  />
                </label>
                <div className="row settings-token-actions">
                  <button
                    type="button"
                    className="ghost"
                    disabled={busy || mock}
                    onClick={() => copyWorkerSetupCmd("linux")}
                    title="Copy Linux setup one-liner"
                  >
                    Copy Linux
                  </button>
                </div>
                <label>
                  Windows (PowerShell)
                  <textarea
                    className="worker-setup-cmd"
                    readOnly
                    rows={3}
                    value={windowsWorkerSetupCmd()}
                    spellCheck={false}
                  />
                </label>
                <div className="row settings-token-actions">
                  <button
                    type="button"
                    className="ghost"
                    disabled={busy || mock}
                    onClick={() => copyWorkerSetupCmd("windows")}
                    title="Copy Windows setup one-liner"
                  >
                    Copy Windows
                  </button>
                  {setupCmdCopyMsg ? <span className="hint">{setupCmdCopyMsg}</span> : null}
                </div>
              </div>

              <div className="worker-connect" aria-label="Connect options">
                <strong className="worker-connect-title">Connect</strong>
                <p className="hint">
                  After the worker is running, put its address in Host / Port (or Advanced full
                  URL), then <strong>Save worker</strong>:
                </p>
                <ul className="worker-setup-steps">
                  <li>
                    <strong>Host IP + port</strong> — usual from in-cluster Studio: this machine&apos;s
                    LAN IP, port <code>{DEFAULT_WORKER_PORT}</code>.
                  </li>
                  <li>
                    <strong>Localhost</strong> — API and worker on the same host →{" "}
                    <em>Use local defaults</em> (
                    <code>
                      {DEFAULT_WORKER_HOST}:{DEFAULT_WORKER_PORT}
                    </code>
                    ).
                  </li>
                  <li>
                    <strong>Full URL</strong> — Advanced → https / remote FQDN (e.g.{" "}
                    <code>https://vllm.dataknife.net</code>).
                  </li>
                </ul>
                <p className="hint">
                  SD / SDXL / Pony / Flux bases belong on the GPU worker — not on a remote text LLM
                  endpoint.
                </p>
              </div>
            </aside>

            <div className="worker-controls-panel" aria-label="Worker controls">
              <div className="settings-block-head">
                <span className="label-with-help">
                  <strong>Controls</strong>
                  <HelpQ topic="help-worker-token" label="Machine token" onOpen={openHelp} />
                </span>
              </div>

              <div className="settings-block worker-auth-block" aria-label="Machine API token">
                <div className="settings-block-head">
                  <strong>Machine API token</strong>
                  <span className={`key-on-file ${settings?.apiTokenConfigured ? "is-on" : ""}`}>
                    {settings?.apiTokenConfigured
                      ? studioToken
                        ? "Configured · in browser"
                        : "Configured"
                      : "Not required (local/dev)"}
                  </span>
                </div>
                <div className="row settings-token-actions">
                  <button
                    type="button"
                    className={`primary btn-save${saveFlash === "token" ? " is-saved" : ""}`}
                    disabled={busy || tokenBusy || mock}
                    onClick={generateApiToken}
                    title="Rotate machine token for MCP / workers"
                  >
                    {tokenBusy
                      ? "Generating…"
                      : saveFlash === "token"
                        ? "Saved ✓"
                        : "Generate machine token"}
                  </button>
                </div>
                {saveFlash === "token" && savedMsg ? (
                  <p className="save-toast" role="status">
                    {savedMsg}
                  </p>
                ) : null}
                {generatedToken && (
                  <div className="token-once">
                    <label>
                      New token (copy now — shown once)
                      <input type="text" readOnly value={generatedToken} autoComplete="off" />
                    </label>
                    <div className="row settings-token-actions">
                      <button type="button" className="ghost" onClick={copyGeneratedToken}>
                        Copy
                      </button>
                      {tokenCopyMsg && <span className="hint">{tokenCopyMsg}</span>}
                    </div>
                  </div>
                )}
              </div>

              <div className="worker-panel" aria-label="Diffusion worker status">
                <div className="worker-panel-head">
                  <span
                    className={`status-dot ${
                      workerStats?.reachable
                        ? "on"
                        : workerStats?.configured || composedWorkerUrl()
                          ? "warn"
                          : ""
                    }`}
                  />
                  <strong>Status</strong>
                  <span className="hint">
                    {mock
                      ? "mock stats"
                      : workerStats
                        ? [
                            workerStats.reachable
                              ? "reachable"
                              : workerStats.configured || composedWorkerUrl()
                                ? "unreachable"
                                : "not configured",
                            workerStats.runtime?.kind && workerStats.runtime.kind !== "none"
                              ? workerStats.runtime.kind
                              : null,
                          ]
                            .filter(Boolean)
                            .join(" · ")
                        : "—"}
                  </span>
                </div>

                <div className="row settings-token-actions">
                  <button
                    type="button"
                    className="ghost"
                    disabled={busy}
                    onClick={applyLocalWorkerDefaults}
                  >
                    Use local defaults
                  </button>
                </div>

                {!useWorkerUrlOverride && (
                  <div className="worker-endpoint-row" aria-label="Worker host and port">
                    <label>
                      Host
                      <input
                        autoComplete="off"
                        placeholder={DEFAULT_WORKER_HOST}
                        value={workerHost}
                        onChange={(e) => setWorkerHost(e.target.value)}
                      />
                    </label>
                    <label className="worker-port-field">
                      Port
                      <input
                        autoComplete="off"
                        inputMode="numeric"
                        placeholder={DEFAULT_WORKER_PORT}
                        value={workerPort}
                        onChange={(e) => setWorkerPort(e.target.value)}
                      />
                    </label>
                  </div>
                )}
                <p className="hint">
                  <code>{composedWorkerUrl() || "(empty)"}</code>
                </p>

                <button
                  type="button"
                  className={`primary btn-save${saveFlash === "worker" ? " is-saved" : ""}`}
                  disabled={busy}
                  onClick={saveWorkerEndpoint}
                >
                  {saveFlash === "worker" ? "Saved ✓" : "Save worker"}
                </button>
                {saveFlash === "worker" && savedMsg ? (
                  <p className="save-toast" role="status">
                    {savedMsg}
                  </p>
                ) : savedMsg ? (
                  <p className="hint">{savedMsg}</p>
                ) : null}

                {(() => {
                  const vram = workerStats?.vram;
                  const used = vram?.usedMiB;
                  const total = vram?.totalMiB;
                  const pct =
                    typeof used === "number" && typeof total === "number" && total > 0
                      ? Math.min(100, Math.round((used / total) * 100))
                      : null;
                  const util = vram?.utilPercent;
                  const storage = workerStats?.storage;
                  return (
                    <div className="worker-meters">
                      <div className="worker-vram">
                        <div className="worker-vram-meta">
                          <span>VRAM</span>
                          <span>
                            {pct != null
                              ? `${used} / ${total} MiB (${pct}%)`
                              : mock
                                ? "— (mock)"
                                : "n/a"}
                            {typeof util === "number" ? ` · GPU ${util}%` : ""}
                          </span>
                        </div>
                        <div
                          className="worker-vram-bar"
                          role="progressbar"
                          aria-valuenow={pct ?? 0}
                          aria-valuemin={0}
                          aria-valuemax={100}
                        >
                          <div className="worker-vram-fill" style={{ width: `${pct ?? 0}%` }} />
                        </div>
                      </div>
                      <StorageMeter
                        label="Gallery PVC"
                        disk={storage?.gallery}
                        fallbackHint={mock ? "— (mock)" : "n/a"}
                      />
                      <StorageMeter
                        label="Models dir"
                        disk={storage?.models}
                        fallbackHint={
                          mock
                            ? "— (mock)"
                            : workerStats?.reachable
                              ? "n/a"
                              : "worker unreachable"
                        }
                      />
                      {(workerStats?.loras != null || workerStats?.checkpoints != null) && (
                        <p className="hint">
                          LoRAs {workerStats.loras ?? "—"} · checkpoints{" "}
                          {workerStats.checkpoints ?? "—"}
                          {workerStats.uptimeSec != null
                            ? ` · up ${workerStats.uptimeSec}s`
                            : ""}
                        </p>
                      )}
                      {workerStats?.modelLoaded?.model && (
                        <p className="hint">
                          Active base: {workerStats.modelLoaded.model}
                          {workerStats.modelLoaded.baseModel
                            ? ` (${workerStats.modelLoaded.baseModel})`
                            : ""}
                          {activeBaseEngineHint(workerStats.modelLoaded, workerStats)}
                        </p>
                      )}
                      {(() => {
                        const dirHint = modelsDirHint(workerStats);
                        return dirHint ? <p className="hint">{dirHint}</p> : null;
                      })()}
                    </div>
                  );
                })()}
                <WorkerBaseModelPanel
                  busy={busy || workerCtlBusy || mock}
                  hasWorker={hasWorker()}
                  api={api}
                  checkpoints={localCheckpoints}
                  active={{ model: activeBasePath, baseModel: localBaseFamily }}
                  onRefresh={() => refreshInstalledAssets({ quiet: true })}
                  onSaved={(msg) => setSavedMsg(msg)}
                  onError={(msg) => setError(msg)}
                />
                <div className="worker-controls">
                  <button
                    type="button"
                    className="ghost"
                    disabled={busy || workerCtlBusy || mock}
                    onClick={() => workerControl("start")}
                    title="Start local docker/systemd worker"
                  >
                    ▶ Start
                  </button>
                  <button
                    type="button"
                    className="ghost"
                    disabled={busy || workerCtlBusy || mock}
                    onClick={() => workerControl("stop")}
                    title="Stop local docker/systemd worker"
                  >
                    ■ Stop
                  </button>
                </div>
              </div>

              <details className="settings-advanced">
                <summary>Advanced</summary>
                <label>
                  <span className="settings-label-row">
                    Machine API token
                    {studioToken ? (
                      <span className="key-on-file is-on">In browser</span>
                    ) : null}
                  </span>
                  <input
                    type="password"
                    autoComplete="off"
                    placeholder="Paste machine token"
                    value={studioToken}
                    onChange={(e) => setStudioToken(e.target.value)}
                  />
                </label>
                <div className="row settings-token-actions">
                  <button
                    type="button"
                    className={`ghost${saveFlash === "token" ? " is-saved" : ""}`}
                    disabled={busy || tokenBusy || mock}
                    onClick={applyStudioToken}
                    title="Save machine token in this browser"
                  >
                    Save token
                  </button>
                </div>
                <label className="settings-check">
                  <input
                    type="checkbox"
                    checked={useWorkerUrlOverride}
                    onChange={(e) => setUseWorkerUrlOverride(e.target.checked)}
                  />
                  Full worker URL
                </label>
                {useWorkerUrlOverride && (
                  <label>
                    Worker URL
                    <input
                      placeholder="https://vllm.dataknife.net"
                      value={workerUrlOverride}
                      onChange={(e) => setWorkerUrlOverride(e.target.value)}
                    />
                  </label>
                )}
              </details>
            </div>
          </div>
        </section>
      )}

      {tab === "metrics" && (
        <section className="panel settings metrics-panel" aria-label="OpenRouter metrics">
          <div className="settings-block-head">
            <span className="label-with-help">
              <strong>OpenRouter usage</strong>
              <HelpQ topic="help-metrics" label="Metrics" onOpen={openHelp} />
            </span>
            <button
              type="button"
              className="ghost"
              disabled={orMetricsBusy || busy}
              onClick={refreshOpenRouterMetrics}
            >
              {orMetricsBusy ? "Refreshing…" : "Refresh"}
            </button>
          </div>
          {!settings?.openRouterConfigured ? (
            <p className="hint">
              Add an OpenRouter key in{" "}
              <button type="button" className="linkish" onClick={() => setTab("settings")}>
                Settings
              </button>
              .
            </p>
          ) : orMetricsBusy && !orMetrics ? (
            <p className="hint">Loading…</p>
          ) : orMetrics ? (
            <>
              {orMetrics.key ? (
                <div className="metrics-grid" aria-label="Per-key usage">
                  <div className="metric-card">
                    <span className="metric-label">Spend today</span>
                    <span className="metric-value">
                      {fmtUsd(orMetrics.key.usageDaily)}
                    </span>
                  </div>
                  <div className="metric-card">
                    <span className="metric-label">This week</span>
                    <span className="metric-value">
                      {fmtUsd(orMetrics.key.usageWeekly)}
                    </span>
                  </div>
                  <div className="metric-card">
                    <span className="metric-label">This month</span>
                    <span className="metric-value">
                      {fmtUsd(orMetrics.key.usageMonthly)}
                    </span>
                  </div>
                  <div className="metric-card">
                    <span className="metric-label">All-time (this key)</span>
                    <span className="metric-value">{fmtUsd(orMetrics.key.usage)}</span>
                  </div>
                  <div className="metric-card">
                    <span className="metric-label">Key limit remaining</span>
                    <span className="metric-value">
                      {orMetrics.key.limitRemaining == null
                        ? "Unlimited"
                        : fmtUsd(orMetrics.key.limitRemaining)}
                    </span>
                    {orMetrics.key.limit != null ? (
                      <span className="hint">
                        Cap {fmtUsd(orMetrics.key.limit)}
                        {orMetrics.key.limitReset ? ` · resets ${orMetrics.key.limitReset}` : ""}
                      </span>
                    ) : null}
                  </div>
                  {orMetrics.key.label ? (
                    <p className="hint metric-key-label">Key: {orMetrics.key.label}</p>
                  ) : null}
                </div>
              ) : (
                <p className="err">{orMetrics.keyError || "Could not load key usage."}</p>
              )}

              <div className="settings-block" aria-label="Account credits">
                <div className="settings-block-head">
                  <strong>Account credits</strong>
                </div>
                {orMetrics.credits ? (
                  <div className="metrics-grid">
                    <div className="metric-card">
                      <span className="metric-label">Remaining</span>
                      <span className="metric-value">
                        {fmtUsd(orMetrics.credits.remaining)}
                      </span>
                    </div>
                    <div className="metric-card">
                      <span className="metric-label">Purchased</span>
                      <span className="metric-value">
                        {fmtUsd(orMetrics.credits.totalCredits)}
                      </span>
                    </div>
                    <div className="metric-card">
                      <span className="metric-label">Used (lifetime)</span>
                      <span className="metric-value">
                        {fmtUsd(orMetrics.credits.totalUsage)}
                      </span>
                    </div>
                  </div>
                ) : (
                  <p className="hint">
                    {orMetrics.creditsError || "Needs an OpenRouter Management API key."}
                  </p>
                )}
              </div>

              {Array.isArray(orMetrics.notes) && orMetrics.notes.length > 0 ? (
                <ul className="metrics-notes hint">
                  {orMetrics.notes.map((n) => (
                    <li key={n}>{n}</li>
                  ))}
                </ul>
              ) : null}
            </>
          ) : (
            <p className="hint">No metrics yet — hit Refresh.</p>
          )}
        </section>
      )}

      {tab === "settings" && (
        <section className="panel settings">
          <p className="hint">
            {mock
              ? "Mock mode: keys are fake flags in memory. Paste anything non-empty to “configure” Civitai/OpenRouter. Agent/image models live in the compose chrome (Mode · Agent · Image)."
              : "Keys stay on the Freya API (or in your Kubernetes Secrets). They are never shown back in full after save."}{" "}
            OpenRouter alone is enough for cloud Generate — a worker is only for LoRAs and custom
            local looks.
          </p>
          <label>
            Studio API token
            <input
              type="password"
              autoComplete="off"
              placeholder="Bearer for /v1 when Freya API auth is enabled"
              value={studioToken}
              onChange={(e) => setStudioToken(e.target.value)}
            />
            <span className="hint">
              Stored only in this browser (localStorage). Required when the API has{" "}
              <code>FREYA_API_TOKEN</code> / <code>apiTokenSecretRef</code> set. Leave blank for open
              local/dev.
            </span>
          </label>

          <div className="settings-block" aria-label="OpenRouter privacy">
            <div className="settings-block-head">
              <span className="label-with-help">
                <strong>Privacy</strong>
                <HelpQ topic="help-zdr" label="Privacy / ZDR" onOpen={openHelp} />
              </span>
            </div>
            <label>
              Data collection
              <select
                value={privacyDataCollection}
                onChange={(e) => setPrivacyDataCollection(e.target.value)}
              >
                <option value="allow">Allow</option>
                <option value="deny">Deny</option>
              </select>
            </label>
            <label className="settings-check">
              <input
                type="checkbox"
                checked={privacyZdr}
                onChange={(e) => setPrivacyZdr(e.target.checked)}
              />
              ZDR
            </label>
          </div>

          <div className="settings-block" aria-label="Instruction tone">
            <div className="settings-block-head">
              <span className="label-with-help">
                <strong>Tone</strong>
                <HelpQ topic="help-tone" label="Instruction tone" onOpen={openHelp} />
              </span>
            </div>
            <select
              aria-label="Tone"
              value={instructionTone}
              onChange={(e) => setInstructionTone(e.target.value)}
            >
              <option value="warm">Warm</option>
              <option value="direct">Direct</option>
              <option value="steerable">Steerable</option>
            </select>
          </div>

          <div className="settings-block" aria-label="System prompts">
            <div className="settings-block-head">
              <span className="label-with-help">
                <strong>System prompts</strong>
                <HelpQ topic="help-prompts" label="System prompts" onOpen={openHelp} />
              </span>
            </div>
            <select
              aria-label="System prompts"
              value={instructionMode}
              onChange={(e) => {
                const id = e.target.value;
                setInstructionMode(id);
                const row = instructions.find((i) => i.id === id);
                setInstructionDraft(row?.text || "");
              }}
            >
              {(instructions.length
                ? instructions
                : [
                    { id: "converse", label: "Converse (chat agent)" },
                    { id: "route", label: "Route (intent classifier)" },
                    { id: "chat", label: "Chat (assist)" },
                    { id: "plan", label: "Plan" },
                    { id: "suggest", label: "Suggest" },
                    { id: "session", label: "Session" },
                    { id: "prompt-prose", label: "Enhance — prose brief" },
                    { id: "prompt-sd", label: "Enhance — SD keywords" },
                  ]
              ).map((i) => (
                <option key={i.id} value={i.id}>
                  {i.label || i.id}
                  {i.overridden ? " · overridden" : ""}
                </option>
              ))}
            </select>
            <label>
              System instruction
              <textarea
                className="settings-prompt-textarea"
                rows={10}
                value={instructionDraft}
                onChange={(e) => setInstructionDraft(e.target.value)}
                spellCheck={false}
              />
            </label>
            <div className="settings-key-actions">
              <button
                type="button"
                className={`primary${saveFlash === "settings" ? " is-saved" : ""}`}
                disabled={instructionBusy || busy}
                onClick={saveInstructionPrompt}
              >
                {instructionBusy
                  ? "Saving…"
                  : saveFlash === "settings"
                    ? "Saved ✓"
                    : "Save prompt"}
              </button>
              <button
                type="button"
                disabled={instructionBusy || busy}
                onClick={resetInstructionPrompt}
              >
                Reset to default
              </button>
              {instructions.find((i) => i.id === instructionMode)?.overridden ? (
                <span className="hint">Custom</span>
              ) : (
                <span className="hint">Default</span>
              )}
            </div>
          </div>

          {saveFlash === "settings" && savedMsg ? (
            <p className="save-toast" role="status">
              {savedMsg}
            </p>
          ) : savedMsg && tab === "settings" ? (
            <p className="hint">{savedMsg}</p>
          ) : null}
        </section>
      )}

      {tab === "models" && (
        <section className="panel settings" aria-label="Models">
          <div className="settings-block" aria-label="Composer defaults">
            <div className="settings-block-head">
              <span className="label-with-help">
                <strong>Composer defaults</strong>
                <HelpQ topic="help-composer" label="Composer defaults" onOpen={openHelp} />
              </span>
            </div>
            <label>
              Agent
              <select
                value={agentMenuValue}
                onChange={(e) => changeAgentModel(e.target.value)}
              >
                {agentOptions.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.label || m.id}
                  </option>
                ))}
              </select>
            </label>
            <label>
              Image
              <select
                value={imageMenuValue}
                onChange={(e) => changeImageModel(e.target.value)}
              >
                {imageOptions.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.label || m.id}
                  </option>
                ))}
              </select>
            </label>
            <label>
              Video
              <select
                value={videoMenuValue}
                onChange={(e) => changeVideoModel(e.target.value)}
              >
                {(videoOptions.length
                  ? videoOptions
                  : [{ id: AUTO_VIDEO.id, label: AUTO_VIDEO.label || "Auto" }]
                ).map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.label || m.id}
                  </option>
                ))}
              </select>
            </label>
            <label>
              Router
              <select
                value={routerMenuValue}
                onChange={(e) => changeRouterModel(e.target.value)}
              >
                {routerOptions.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.label || m.id}
                  </option>
                ))}
              </select>
            </label>
            <label>
              <span className="label-with-help">
                Backend
                <HelpQ topic="help-backend" label="Cloud | Local" onOpen={openHelp} />
              </span>
              <select
                value={generateBackend}
                onChange={(e) => changeGenerateBackend(e.target.value)}
              >
                <option value="cloud">Cloud</option>
                <option value="local" disabled={!localBackendOk}>
                  Local
                </option>
              </select>
            </label>
          </div>

          <div className="settings-block" aria-label="Auto fallbacks">
            <div className="settings-block-head">
              <span className="label-with-help">
                <strong>Auto fallbacks</strong>
                <HelpQ topic="help-auto" label="Auto fallbacks" onOpen={openHelp} />
              </span>
              <button
                type="button"
                className="ghost"
                disabled={busy}
                onClick={() =>
                  setAutoFallbacks((prev) =>
                    normalizeAutoFallbacks({
                      ...EMPTY_AUTO_FALLBACKS,
                      costTier: prev.costTier || "medium",
                    }),
                  )
                }
              >
                Reset
              </button>
            </div>
            {[
              ["agent", "Agent"],
              ["agentZdr", "Agent (ZDR)"],
              ["image", "Image"],
              ["imageZdr", "Image (ZDR)"],
              ["video", "Video"],
              ["videoZdr", "Video (ZDR)"],
              ["router", "Router"],
            ].map(([key, label]) => (
              <label key={key}>
                {label}
                <input
                  type="text"
                  list={`auto-fb-${key}`}
                  placeholder="Built-in (blank)"
                  value={autoFallbacks[key] || ""}
                  onChange={(e) =>
                    setAutoFallbacks((prev) =>
                      normalizeAutoFallbacks({ ...prev, [key]: e.target.value }),
                    )
                  }
                  spellCheck={false}
                />
                <datalist id={`auto-fb-${key}`}>
                  {(key.startsWith("image")
                    ? imageOptions
                    : key.startsWith("video")
                      ? videoOptions
                      : agentOptions
                  )
                    .filter((m) => m.id && m.id !== AUTO_AGENT.id && m.id !== AUTO_IMAGE.id && m.id !== AUTO_VIDEO.id && m.id !== AUTO_ROUTER.id)
                    .slice(0, 40)
                    .map((m) => (
                      <option key={m.id} value={m.id}>
                        {m.label || m.id}
                      </option>
                    ))}
                </datalist>
              </label>
            ))}
          </div>

          <div className="settings-block" aria-label="Catalog alignment">
            <div className="settings-block-head">
              <span className="label-with-help">
                <strong>Alignment</strong>
                <HelpQ topic="help-alignment" label="Catalog alignment" onOpen={openHelp} />
              </span>
            </div>
            <label>
              Preference
              <select
                value={modelAlignment}
                onChange={(e) => setModelAlignment(e.target.value)}
              >
                <option value="balanced">Balanced</option>
                <option value="safety">Safety</option>
                <option value="uncensored">Uncensored</option>
              </select>
            </label>
          </div>

          <div className="settings-block" aria-label="Chat providers">
            <div className="settings-block-head">
              <span className="label-with-help">
                <strong>Chat providers</strong>
                <HelpQ topic="help-providers" label="Chat providers" onOpen={openHelp} />
              </span>
              {(() => {
                const total = chatProviderAvailable.length;
                const on = chatProviderAvailable.filter((s) =>
                  chatProviderIsChecked(s, chatProviders),
                ).length;
                return (
                  <span className="hint">
                    {total ? `${on} / ${total}` : "—"}
                  </span>
                );
              })()}
            </div>
            <div className="chat-provider-toolbar">
              <input
                type="search"
                placeholder="Search vendors…"
                value={chatProviderSearch}
                onChange={(e) => setChatProviderSearch(e.target.value)}
                aria-label="Search chat providers"
              />
              <span className="settings-token-actions">
                <button
                  type="button"
                  className="ghost"
                  disabled={!chatProviderAvailable.length || busy}
                  onClick={() => setChatProviders(chatProvidersSelectAll(chatProviderAvailable, true))}
                >
                  Select all
                </button>
                <button
                  type="button"
                  className="ghost"
                  disabled={!chatProviderAvailable.length || busy}
                  onClick={() => setChatProviders(chatProvidersSelectAll(chatProviderAvailable, false))}
                >
                  Select none
                </button>
              </span>
            </div>
            <div className="chat-provider-list" role="group" aria-label="Vendor checkboxes">
              {chatProviderAvailable.length === 0 ? (
                <span className="hint">No vendors yet.</span>
              ) : (
                chatProviderAvailable
                  .filter((slug) => {
                    const q = chatProviderSearch.trim().toLowerCase();
                    if (!q) return true;
                    return slug.includes(q);
                  })
                  .map((slug) => {
                    const checked = chatProviderIsChecked(slug, chatProviders);
                    return (
                      <label key={slug} className="settings-check chat-provider-row">
                        <input
                          type="checkbox"
                          checked={checked}
                          onChange={(e) =>
                            setChatProviders(
                              chatProvidersAfterToggle(
                                chatProviderAvailable,
                                chatProviders,
                                slug,
                                e.target.checked,
                              ),
                            )
                          }
                        />
                        <span>{slug}</span>
                      </label>
                    );
                  })
              )}
            </div>
            {chatProviders.legacyOnly?.length ? (
              <span className="hint warn-hint">
                Legacy allowlist: {chatProviders.legacyOnly.join(", ")}
              </span>
            ) : null}
          </div>

          {modelsCatalogReady ? (
            <p className="hint">
              Agent {Math.max(0, agentModels.length - 1)} · Image{" "}
              {Math.max(0, imageModels.length - 1)} · Router{" "}
              {Math.max(0, routerModels.length - 1)} · Video{" "}
              {videoModels.length > 0 ? videoModels.length : "—"}
            </p>
          ) : (
            <p className="hint">{MODELS_API_HINT}</p>
          )}
          <button
            type="button"
            className={`primary btn-save${saveFlash === "models" ? " is-saved" : ""}`}
            disabled={busy}
            onClick={saveSettings}
          >
            {saveFlash === "models" || saveFlash === "settings" ? "Saved ✓" : "Save models"}
          </button>
          {(saveFlash === "models" || saveFlash === "settings") && savedMsg ? (
            <p className="save-toast" role="status">
              {savedMsg}
            </p>
          ) : null}
        </section>
      )}

      {tab === "help" && (
        <section className="panel settings help-panel" aria-label="Help">
          <h2 className="help-heading">Help</h2>
          <p className="help-lead">
            Short notes for Studio controls. Open a section from any <span className="help-q-inline">?</span>{" "}
            button, or jump below.
          </p>
          <nav className="help-toc" aria-label="Help sections">
            {[
              ["help-overview", "Overview"],
              ["help-civitai", "Civitai"],
              ["help-civitai-downloads", "Downloads"],
              ["help-worker", "Worker"],
              ["help-worker-token", "Machine token"],
              ["help-keys", "API keys"],
              ["help-nsfw", "NSFW"],
              ["help-zdr", "Privacy / ZDR"],
              ["help-tone", "Tone"],
              ["help-prompts", "System prompts"],
              ["help-composer", "Composer"],
              ["help-backend", "Cloud | Local"],
              ["help-cost", "Cost"],
              ["help-auto", "Auto fallbacks"],
              ["help-alignment", "Alignment"],
              ["help-providers", "Chat providers"],
              ["help-metrics", "Metrics"],
              ["help-modes", "Plan / Auto / YOLO"],
            ].map(([id, label]) => (
              <button
                key={id}
                type="button"
                className="linkish"
                onClick={() => openHelp(id)}
              >
                {label}
              </button>
            ))}
          </nav>

          <HelpSection id="help-overview" title="Overview" active={helpFocus === "help-overview"}>
            <p>
              Chat on the left; Gallery / Styles / Civitai / Worker on the tabs. Cloud Generate needs
              an OpenRouter key. Local LoRAs and Local generate need a diffusion worker.
            </p>
            <p>
              Keys and Privacy live under Settings. Model pickers, Auto fallbacks, and chat vendors
              live under Models.
            </p>
          </HelpSection>

          <HelpSection id="help-civitai" title="Civitai" active={helpFocus === "help-civitai"}>
            <p>
              <strong>Search</strong> finds LoRAs / checkpoints (needs a Civitai API key).{" "}
              <strong>★ Favorite</strong> is a persistent library in this browser.{" "}
              <strong>Pin</strong> is session-only for LoRAs / adapters / embeddings — not
              checkpoints (use Worker → Set as base). <strong>Install</strong> queues a download
              and does not auto-pin.
            </p>
            <p>
              <strong>Installed</strong> shows assets already on the worker disk as clickable cards
              (same style as Favorites) — open a card for details, pin (adapters only), delete, or
              set base.
            </p>
            <p>
              Chat search results open on <strong>Styles</strong> as a scrollable table: installed
              matches first, then other Civitai hits, one asset per row. Chat only says what it
              found.
            </p>
          </HelpSection>

          <HelpSection id="help-civitai-downloads" title="Downloads" active={helpFocus === "help-civitai-downloads"}>
            <p>
              Background install queue on the worker (up to a few downloads in parallel). Progress
              shows bytes / %. Cancel applies to queued or in-flight jobs. <strong>Clear</strong>{" "}
              removes finished jobs (succeeded / failed / cancelled) from the list only — it does
              not delete files or cancel active downloads.
            </p>
          </HelpSection>

          <HelpSection id="help-worker" title="Worker" active={helpFocus === "help-worker"}>
            <p>
              Primary setup lives on the <strong>Worker</strong> tab: one-line Linux/Windows host
              scripts (machine token included), then Host/Port → Save worker. Short reminder: GPU
              process is <code>freya-local-worker</code> (default port {DEFAULT_WORKER_PORT}); Studio
              is usually in-cluster and reaches it by URL (
              <code>FreyaDiffusionWorker</code> <code>externalEndpoint</code>).
            </p>
          </HelpSection>

          <HelpSection id="help-worker-token" title="Machine API token" active={helpFocus === "help-worker-token"}>
            <p>
              Service token for MCP / workers (not human sign-in). Generate rotates the token; copy
              it once. Advanced → paste / Save token stores it in this browser for recovery.
            </p>
          </HelpSection>

          <HelpSection id="help-keys" title="API keys" active={helpFocus === "help-keys"}>
            <p>
              Keys stay on the Freya API (or Kubernetes Secrets). After save they are never shown
              back in full — empty field + “configured” badge means still on file. OpenRouter alone
              is enough for cloud Generate. Civitai is for search / install.
            </p>
            {mock ? (
              <p>Mock mode: any non-empty value counts as configured.</p>
            ) : null}
          </HelpSection>

          <HelpSection id="help-nsfw" title="Show NSFW local assets" active={helpFocus === "help-nsfw"}>
            <p>
              When off, Freya hides NSFW-looking local and Civitai results by name / API flag. Turn
              on to show them in Studio lists.
            </p>
          </HelpSection>

          <HelpSection id="help-zdr" title="Privacy / ZDR" active={helpFocus === "help-zdr"}>
            <p>
              <strong>ZDR</strong> asks OpenRouter for zero-data-retention routes on Freya requests
              (<code>provider.zdr</code>). Account-wide ZDR at{" "}
              <a href="https://openrouter.ai/settings/privacy" target="_blank" rel="noreferrer">
                openrouter.ai/settings/privacy
              </a>{" "}
              still applies — Freya can only tighten routing, not loosen it.
            </p>
            <p>
              With ZDR on, Image / Video pickers filter to ZDR-capable models and Auto prefers
              Google media models. Many Flux / Seedream endpoints have no ZDR route and fail with a
              clear error instead of a silent dead-end.
            </p>
            <p>
              <strong>Data collection</strong> — Allow (providers may store/train) or Deny (only
              no-collection endpoints).
            </p>
          </HelpSection>

          <HelpSection id="help-tone" title="Instruction tone" active={helpFocus === "help-tone"}>
            <p>
              Appends a tone hint after pack / custom system prompts: Warm (default), Direct
              (concise), or Steerable (low-refusal creative).
            </p>
          </HelpSection>

          <HelpSection id="help-prompts" title="System prompts" active={helpFocus === "help-prompts"}>
            <p>
              Override Freya pack instructions (<code>packs/instructions/*.txt</code>).{" "}
              <strong>Route</strong> classifies chat/picture/gif/video and Auto models;{" "}
              <strong>Converse</strong> is the chat agent (tools, Installed-first LoRAs,
              Plan/Auto/YOLO). Empty override uses the shipped default. Tone still appends after
              the text. Reset clears the override.
            </p>
          </HelpSection>

          <HelpSection id="help-composer" title="Composer defaults" active={helpFocus === "help-composer"}>
            <p>
              Pins for Agent / Image / Video / Router. <strong>Auto</strong> is Freya’s internal
              router: it picks a concrete model for that job. If it fails, the cost / privacy /
              alignment fallback is used. The $ button on the prompt bar opens Cost (Low /
              Medium / High / Max) and is the same saved preference Auto uses. <strong>OpenRouter router (upstream)</strong> is{" "}
              <code>openrouter/auto</code> — only when you select that row, not when you pick Auto.
            </p>
          </HelpSection>

          <HelpSection id="help-cost" title="Cost" active={helpFocus === "help-cost"}>
            <p>
              The $ button on the prompt bar opens Low, Medium, High, or Max and chooses which
              built-in model the internal router and Auto fallbacks use. Medium is the default.
              The choice is the saved cost preference. Privacy (ZDR / data collection) and
              Alignment still change the set. A filled Auto fallback box on the Models tab
              overrides that one cell.
            </p>
          </HelpSection>

          <HelpSection id="help-backend" title="Cloud | Local" active={helpFocus === "help-backend"}>
            <p>
              Same switch as the composer bar (stored in this browser). Cloud = OpenRouter. Local =
              GPU worker (needs a reachable worker URL).
            </p>
          </HelpSection>

          <HelpSection id="help-auto" title="Auto fallbacks" active={helpFocus === "help-auto"}>
            <p>
              Concrete models when Agent / Image / Video / Router are Auto and the internal router
              does not suggest one. Leave blank to use the built-in set for the current cost tier,
              privacy, and alignment. ZDR rows apply when ZDR or deny-collection is on. Do not put
              openrouter/auto here — that id is only the explicit upstream picker option.
            </p>
          </HelpSection>

          <HelpSection id="help-alignment" title="Catalog alignment" active={helpFocus === "help-alignment"}>
            <p>
              Freya-side filter on <code>/v1/models</code> (OpenRouter moderation flags + name
              heuristics): Balanced (full list), Safety (moderated / major vendors), or Uncensored
              (prefer low-moderation). Also steers blank Auto fallbacks.
            </p>
          </HelpSection>

          <HelpSection id="help-providers" title="Chat providers" active={helpFocus === "help-providers"}>
            <p>
              Checked vendors appear in the Agent / Router picker. Unchecked are hidden. New vendors
              from OpenRouter default to checked (opt-out).
            </p>
          </HelpSection>

          <HelpSection id="help-metrics" title="OpenRouter metrics" active={helpFocus === "help-metrics"}>
            <p>
              Per-key spend from Freya’s completion key. Account credits need an OpenRouter
              Management API key — see{" "}
              <a
                href="https://openrouter.ai/docs/guides/overview/auth/management-api-keys"
                target="_blank"
                rel="noreferrer"
              >
                Management keys
              </a>
              .
            </p>
          </HelpSection>

          <HelpSection id="help-modes" title="Plan / Auto / YOLO" active={helpFocus === "help-modes"}>
            <p>
              <strong>Plan</strong> — Freya proposes tool calls; confirm before they run.{" "}
              <strong>Auto</strong> — runs tools, asks when installs or character/LoRA choice are
              ambiguous. <strong>YOLO</strong> — picks and runs (generate, search, install when a
              worker is set). For character likeness, Freya prefers Installed base+LoRA combos
              before Civitai.
            </p>
          </HelpSection>
        </section>
      )}

      {tab === "guidelines" && (
        <section className="panel settings guidelines-panel" aria-label="User guidelines">
          <h2 className="guidelines-heading">Guidelines</h2>
          <p className="guidelines-lead">
            Freya is yours to run — with that comes responsibility for prompts, downloads, and
            anything you publish.
          </p>
          <GuidelinesBody />
          {!guidelinesAcked ? (
            <div className="login-actions">
              <button type="button" className="primary" onClick={acceptGuidelines}>
                Accept &amp; acknowledge
              </button>
            </div>
          ) : (
            <p className="hint">Acknowledged in this browser.</p>
          )}
        </section>
      )}
        </section>
      </div>
      </div>

      {civitaiDetail && detailModel && (
        <div
          className="civitai-modal-backdrop"
          role="presentation"
          onClick={closeCivitaiDetail}
        >
          <div
            className="civitai-modal"
            role="dialog"
            aria-modal="true"
            aria-label={detailModel.name || "Civitai asset"}
            onClick={(e) => e.stopPropagation()}
          >
            <div className="civitai-modal-head">
              <div>
                <span className="lora-type-badge">{detailModel.modelType || "Asset"}</span>
                <h3>{detailModel.name}</h3>
                <p className="hint">
                  {[detailModel.creator && `by ${detailModel.creator}`, detailModel.baseModel, detailModel.versionName]
                    .filter(Boolean)
                    .join(" · ")}
                </p>
              </div>
              <button type="button" className="ghost" onClick={closeCivitaiDetail}>
                Close
              </button>
            </div>
            <div className="civitai-modal-gallery">
              {detailImages.length > 0 ? (
                <>
                  <img
                    src={detailImages[Math.min(detailImageIdx, detailImages.length - 1)]}
                    alt=""
                    onError={(e) => e.currentTarget.classList.add("is-broken")}
                  />
                  {detailImages.length > 1 && (
                    <div className="row civitai-modal-thumbs">
                      {detailImages.map((url, i) => (
                        <button
                          key={url}
                          type="button"
                          className={i === detailImageIdx ? "primary" : "ghost"}
                          onClick={() => setDetailImageIdx(i)}
                        >
                          <img src={url} alt="" />
                        </button>
                      ))}
                    </div>
                  )}
                </>
              ) : (
                <div className="lora-card-thumb-fallback">No preview</div>
              )}
            </div>
            <div className="civitai-modal-body">
              {(civitaiDetailBusy || civitaiDetail.loading) && (
                <p className="hint">Loading details…</p>
              )}
              {civitaiDetail.error && <p className="hint">{civitaiDetail.error}</p>}
              {detailModel.needsBase && (
                <p className="civitai-needs-base">{detailModel.needsBase}</p>
              )}
              {detailModel.description && <p>{detailModel.description}</p>}
              {(detailModel.trainedWords?.length || detailModel.triggerWord) && (
                <p className="hint">
                  Triggers:{" "}
                  {(detailModel.trainedWords?.length
                    ? detailModel.trainedWords
                    : [detailModel.triggerWord]
                  ).filter(Boolean).join(" · ")}
                </p>
              )}
              {(detailModel.stats || detailModel.downloadCount != null) && (
                <p className="hint">
                  {[
                    formatStat(detailModel.stats?.downloadCount ?? detailModel.downloadCount) &&
                      `${formatStat(detailModel.stats?.downloadCount ?? detailModel.downloadCount)} downloads`,
                    formatStat(detailModel.stats?.thumbsUpCount ?? detailModel.thumbsUp) &&
                      `${formatStat(detailModel.stats?.thumbsUpCount ?? detailModel.thumbsUp)} likes`,
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </p>
              )}
              {detailVersions.length > 0 && (
                <div className="civitai-variant-picks" aria-label="Version and file">
                  <label>
                    Version
                    <select
                      value={detailSelectedVersion?.id ?? detailModel.versionId ?? ""}
                      onChange={(e) => {
                        const vid = Number(e.target.value);
                        const ver =
                          detailVersions.find((v) => String(v.id) === String(vid)) ||
                          detailVersions[0];
                        const primary = pickPrimaryCivitaiFile(ver?.files || []);
                        selectCivitaiVariant(vid, primary?.id);
                      }}
                    >
                      {detailVersions.map((v) => (
                        <option key={v.id} value={v.id}>
                          {v.name}
                          {v.baseModel ? ` · ${v.baseModel}` : ""}
                        </option>
                      ))}
                    </select>
                  </label>
                  {detailFiles.length > 0 && (
                    <label>
                      File
                      <select
                        value={detailModel.fileId ?? pickPrimaryCivitaiFile(detailFiles)?.id ?? ""}
                        onChange={(e) => {
                          const fid = Number(e.target.value);
                          selectCivitaiVariant(
                            detailSelectedVersion?.id ?? detailModel.versionId,
                            fid,
                          );
                        }}
                      >
                        {detailFiles.map((f) => {
                          const size = formatCivitaiFileSize(f);
                          const fmt = f.format || f.type || "";
                          return (
                            <option key={f.id ?? f.name} value={f.id}>
                              {[f.name || "file", fmt, f.primary ? "primary" : "", size]
                                .filter(Boolean)
                                .join(" · ")}
                            </option>
                          );
                        })}
                      </select>
                    </label>
                  )}
                  {(detailModel.fileName || detailModel.fileFormat) && (
                    <p className="hint">
                      Installs as{" "}
                      <code>{destPathForAsset(detailModel)}</code>
                    </p>
                  )}
                </div>
              )}
              {(civitaiDetail.localOnly || detailModel.localAsset || detailModel.path) && (
                <p className="hint">
                  {[
                    detailModel.path && `Path: ${detailModel.path}`,
                    detailModel.baseModel && `Base: ${detailModel.baseModel}`,
                    detailModel.sizeBytes != null && `Size: ${formatBytes(detailModel.sizeBytes)}`,
                    detailModel.kind && `Kind: ${detailModel.kind}`,
                    detailModel.modelId != null && `Civitai model: ${detailModel.modelId}`,
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </p>
              )}
              <div className="row lora-card-actions">
                <button
                  type="button"
                  className={`${isFavorite(detailModel) ? "primary" : "ghost"} icon-btn`}
                  title={
                    isFavorite(detailModel)
                      ? "Remove from favorites"
                      : "Save to favorites library"
                  }
                  aria-label={isFavorite(detailModel) ? "Unfavorite" : "Favorite"}
                  onClick={() => toggleFavorite(detailModel)}
                >
                  <IconStar filled={isFavorite(detailModel)} />
                </button>
                {isPinnableAsset(detailModel) ? (
                  <button
                    type="button"
                    className={`${isPinned(detailModel) ? "primary" : "ghost"} icon-btn`}
                    title={
                      isPinned(detailModel) ? "Unpin from this session" : "Pin to session"
                    }
                    aria-label={
                      isPinned(detailModel) ? "Unpin from session" : "Pin to session"
                    }
                    onClick={() =>
                      isPinned(detailModel)
                        ? unpinLora(detailModel)
                        : pinLora(
                            detailModel,
                            detailModel.path ? { path: detailModel.path } : undefined,
                          )
                    }
                  >
                    <IconPin />
                  </button>
                ) : null}
                {renderInstallAction(detailModel)}
                {(() => {
                  const installed =
                    findInstalledForItem(detailModel) ||
                    (detailModel.path
                      ? installedAssets.find((a) => a.path === detailModel.path)
                      : null);
                  const isCkpt =
                    installed &&
                    (installed.kind === "checkpoint" ||
                      (installed.path || "").startsWith("checkpoints/"));
                  const isActive = installed && activeBasePath && activeBasePath === installed.path;
                  if (!isCkpt) return null;
                  return (
                    <button
                      type="button"
                      className={isActive ? "primary" : "ghost"}
                      disabled={isActive}
                      onClick={() => setInstalledAsBase(installed)}
                    >
                      {isActive ? "Active base" : "Set as base"}
                    </button>
                  );
                })()}
                {detailModel.civitaiUrl && (
                  <a
                    className="ghost"
                    href={detailModel.civitaiUrl}
                    target="_blank"
                    rel="noreferrer"
                  >
                    Open on Civitai
                  </a>
                )}
              </div>
              {(detailModel.needsBase || detailModel.baseModel) && (
                <p className="hint">
                  {detailModel.needsBase ||
                    `LoRA base family: ${detailModel.baseModel}. Install a matching checkpoint to checkpoints/ on the local worker, then set it as active base on the Worker tab.`}
                </p>
              )}
              {detailRelated.length > 0 && (
                <div className="civitai-related">
                  <h4>Needs checkpoint / related</h4>
                  <p className="hint">
                    Civitai does not expose a hard dependency graph — Freya suggests popular
                    checkpoints for this base family so you can install and set one as active base.
                    Install puts weights under FREYA_MODELS_DIR (game drive / PVC); set active base
                    after.
                  </p>
                  <div className="lora-row">
                    {detailRelated.map((rel) => {
                      const card = rel?.model || rel;
                      return renderLoraCard(
                        {
                          ...card,
                          _relationNote: rel?.note || rel?.relation,
                        },
                        { compact: true },
                      );
                    })}
                  </div>
                </div>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

/** Settings panel: list worker checkpoints + set active base for LoRA-compatible gens.
 * Checkpoint rows come from the parent refresh so install/delete update this select
 * without a second mount-only fetch.
 */
function WorkerBaseModelPanel({
  busy,
  hasWorker,
  api,
  checkpoints,
  active,
  onRefresh,
  onSaved,
  onError,
}) {
  const [catalog, setCatalog] = useState([]);
  const [selected, setSelected] = useState(active?.model || "");
  const [loading, setLoading] = useState(false);
  const activeModel = typeof active?.model === "string" ? active.model : "";
  const prevActiveRef = useRef(activeModel);

  useEffect(() => {
    if (!hasWorker) {
      setCatalog([]);
      return undefined;
    }
    let cancelled = false;
    api("/v1/base-models", { cache: "no-store" })
      .catch(() => ({ items: [] }))
      .then((models) => {
        if (!cancelled) setCatalog(Array.isArray(models?.items) ? models.items : []);
      });
    return () => {
      cancelled = true;
    };
  }, [hasWorker, api]);

  useEffect(() => {
    const activeChanged = prevActiveRef.current !== activeModel;
    prevActiveRef.current = activeModel;
    setSelected((prev) => {
      const paths = (checkpoints || []).map((c) => c.path);
      if (activeChanged && activeModel) return activeModel;
      if (prev && paths.includes(prev)) return prev;
      if (activeModel) return activeModel;
      return "";
    });
  }, [checkpoints, activeModel]);

  const refresh = async () => {
    setLoading(true);
    try {
      await onRefresh?.();
    } catch (e) {
      onError?.(e.message);
    } finally {
      setLoading(false);
    }
  };

  const setBase = async () => {
    if (!selected.trim()) return;
    setLoading(true);
    try {
      const match = (checkpoints || []).find((c) => c.path === selected);
      const catalogHit = catalog.find((c) => c.suggestedModel === selected);
      const body = {
        model: selected.trim(),
        baseModel: match?.baseModel || catalogHit?.baseModel || undefined,
      };
      const res = await api("/v1/worker/base-model", {
        method: "PUT",
        body: JSON.stringify(body),
      });
      if (res?.model) setSelected(res.model);
      onSaved?.(`Active base set to ${res?.model || selected}`);
      await onRefresh?.();
    } catch (e) {
      onError?.(e.message);
    } finally {
      setLoading(false);
    }
  };

  if (!hasWorker) {
    return <p className="hint">Connect a worker to set a base checkpoint.</p>;
  }

  const checkpointRows = checkpoints || [];
  const options = [
    ...checkpointRows.map((c) => ({
      value: c.path,
      label: `${c.name || c.id}${c.baseModel ? ` (${c.baseModel})` : ""}`,
    })),
    ...catalog
      .filter((c) => !checkpointRows.some((x) => x.path === c.suggestedModel))
      .map((c) => ({
        value: c.suggestedModel,
        label: `${c.baseModel} hint → ${c.suggestedModel}`,
      })),
  ];

  return (
    <div className="worker-base-panel">
      <div className="worker-vram-meta">
        <span>Base checkpoint</span>
        <button type="button" className="ghost" disabled={busy || loading} onClick={refresh}>
          Refresh
        </button>
      </div>
      <div className="worker-base-row">
        <select
          value={selected}
          disabled={busy || loading || options.length === 0}
          onChange={(e) => setSelected(e.target.value)}
        >
          {options.length === 0 ? (
            <option value="">No checkpoints yet — install from Civitai</option>
          ) : (
            options.map((o) => (
              <option key={o.value} value={o.value}>
                {o.label}
              </option>
            ))
          )}
        </select>
        <button type="button" className="ghost" disabled={busy || loading || !selected} onClick={setBase}>
          Set active
        </button>
      </div>
      {active?.model && (
        <p className="hint">
          Current: {active.model}
          {active.baseModel ? ` · ${active.baseModel}` : ""}
        </p>
      )}
    </div>
  );
}
