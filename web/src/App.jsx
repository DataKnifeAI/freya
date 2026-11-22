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
const WORKSPACE_SPLIT_STORAGE_KEY = "freya.workspaceSplit";
/** Default / clamp for left chat pane width as % of `.workspace`. */
const WORKSPACE_SPLIT_DEFAULT = 45;
const WORKSPACE_SPLIT_MIN_LEFT_PX = 340;
const WORKSPACE_SPLIT_MIN_RIGHT_PX = 280;
const THEME_STORAGE_KEY = "freya.theme";
const TAB_STORAGE_KEY = "freya.tab";
/** Top-level Studio tabs (nav + restore must stay in sync). */
const STUDIO_TABS = ["gallery", "styles", "civitai", "worker", "settings"];
/** Persistent Civitai library (survives sessions). Distinct from session Pin. */
const FAVORITES_STORAGE_KEY = "freya.civitaiFavorites";
const CIVITAI_SUBVIEW_STORAGE_KEY = "freya.civitaiSubview";
const CIVITAI_SUBVIEWS = ["search", "favorites"];
/** Debounce for Civitai search-as-you-type (ms). */
const CIVITAI_SEARCH_DEBOUNCE_MS = 350;
const CIVITAI_SEARCH_TYPES = "LORA,LoCon,DoRA,Checkpoint,TextualInversion,VAE";
/** Local freya-worker / freya-local-worker defaults (see .env.example, docs/LOCAL.md). */
const DEFAULT_WORKER_HOST = "localhost";
const DEFAULT_WORKER_PORT = "8091";

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

function formatStat(n) {
  if (n == null || Number.isNaN(Number(n))) return null;
  const v = Number(n);
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
  if (v >= 1_000) return `${(v / 1_000).toFixed(1)}K`;
  return String(v);
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

function readStoredTab() {
  try {
    const v = localStorage.getItem(TAB_STORAGE_KEY);
    if (STUDIO_TABS.includes(v)) return v;
  } catch {
    /* ignore */
  }
  return "gallery";
}

function persistTab(tab) {
  try {
    if (STUDIO_TABS.includes(tab)) localStorage.setItem(TAB_STORAGE_KEY, tab);
  } catch {
    /* ignore */
  }
}

function readStoredCivitaiSubview() {
  try {
    const v = localStorage.getItem(CIVITAI_SUBVIEW_STORAGE_KEY);
    if (CIVITAI_SUBVIEWS.includes(v)) return v;
  } catch {
    /* ignore */
  }
  return "search";
}

function persistCivitaiSubview(subview) {
  try {
    if (CIVITAI_SUBVIEWS.includes(subview)) {
      localStorage.setItem(CIVITAI_SUBVIEW_STORAGE_KEY, subview);
    }
  } catch {
    /* ignore */
  }
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
  plan: "Plan — Freya proposes tool calls (generate/install). Confirm before they run.",
  auto: "Auto — Freya runs tools, but asks when installs are ambiguous.",
  yolo: "YOLO — Freya runs tools freely (generate, search, install when a worker is set).",
};

/** Offline / unauth fallbacks — live lists come from GET /v1/models. */
const AUTO_AGENT = { id: "openrouter/auto", label: "Auto", short: "Auto" };
const AUTO_IMAGE = { id: "auto", label: "Auto", short: "Auto" };

const FALLBACK_AGENT_MODELS = [
  AUTO_AGENT,
  { id: "anthropic/claude-sonnet-5.5", label: "Claude Sonnet 5.5", short: "Sonnet 5.5" },
  { id: "google/gemini-4-neon", label: "Gemini 4 Neon", short: "Gemini Neon" },
  { id: "openai/gpt-5.2", label: "GPT-5.2", short: "GPT-5.2" },
];

const FALLBACK_IMAGE_MODELS = [
  AUTO_IMAGE,
  { id: "black-forest-labs/flux-3-image-klein", label: "FLUX 3 Image Klein", short: "FLUX 3 Klein" },
  { id: "google/gemini-2.5-flash-image", label: "Gemini 2.5 Flash Image", short: "Gemini Image" },
  { id: "openai/gpt-image-1-mini", label: "GPT Image 1 Mini", short: "GPT Image Mini" },
];

function shortFromName(name, id) {
  let s = String(name || id || "").trim();
  const colon = s.indexOf(": ");
  if (colon > 0 && colon < 32) s = s.slice(colon + 2).trim();
  if (s.length > 22) s = `${s.slice(0, 20)}…`;
  return s || id;
}

/** Map API catalog rows → CompactMenu options; always keep Auto first. */
function catalogToOptions(items, { auto, fallback }) {
  const mapped = (Array.isArray(items) ? items : [])
    .filter((m) => m && m.id)
    .map((m) => ({
      id: m.id,
      label: m.name || m.id,
      short: shortFromName(m.name, m.id),
    }));
  if (!mapped.length) return fallback;
  return [auto, ...mapped];
}

function ensureSelectedOption(list, id) {
  if (!id || list.some((m) => m.id === id)) return list;
  return [...list, { id, label: id, short: shortFromName(id, id) }];
}

const emptySession = () => ({
  goal: "",
  pinnedLoras: [],
  turns: [],
  messages: [],
  sessionNote: "",
});

/** Normalize persisted session: promote legacy turns into chat messages. */
function normalizeSession(raw) {
  if (!raw || typeof raw !== "object") return emptySession();
  const pinnedLoras = Array.isArray(raw.pinnedLoras) ? raw.pinnedLoras : [];
  const turns = Array.isArray(raw.turns) ? raw.turns : [];
  let messages = Array.isArray(raw.messages) ? raw.messages : [];
  if (!messages.length && turns.length) {
    messages = turns
      .slice()
      .reverse()
      .flatMap((t) => {
        const at = t.at || new Date().toISOString();
        const out = [{ role: "user", content: t.prompt || "", at }];
        if (t.artifactIds?.length) {
          out.push({
            role: "assistant",
            content: `Generated ${t.artifactIds.length} image${t.artifactIds.length === 1 ? "" : "s"}.`,
            at,
            artifactIds: t.artifactIds,
          });
        }
        return out;
      });
  }
  return {
    goal: raw.goal || "",
    pinnedLoras,
    turns,
    messages,
    sessionNote: raw.sessionNote || "",
  };
}

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

function readStoredWorkspaceSplit() {
  try {
    const n = Number(localStorage.getItem(WORKSPACE_SPLIT_STORAGE_KEY));
    if (Number.isFinite(n) && n >= 15 && n <= 85) return n;
  } catch {
    /* ignore */
  }
  return WORKSPACE_SPLIT_DEFAULT;
}

function persistWorkspaceSplit(pct) {
  try {
    localStorage.setItem(WORKSPACE_SPLIT_STORAGE_KEY, String(Math.round(pct * 10) / 10));
  } catch {
    /* ignore */
  }
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
      const agent =
        typeof parsed.agentModel === "string" && parsed.agentModel.trim()
          ? parsed.agentModel.trim()
          : "openrouter/auto";
      const image =
        typeof parsed.imageModel === "string" && parsed.imageModel.trim()
          ? parsed.imageModel.trim()
          : "auto";
      return { agentModel: agent, imageModel: image };
    }
  } catch {
    /* ignore */
  }
  return { agentModel: "openrouter/auto", imageModel: "auto" };
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
const IconSend = () => (
  <Svg>
    <path d="M5 12h14M13 6l6 6-6 6" />
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
  }, [open, onOpen]);

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
  const [tab, setTabState] = useState(readStoredTab);
  const setTab = (next) => {
    setTabState(next);
    persistTab(next);
  };
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
  const [workerHost, setWorkerHost] = useState(DEFAULT_WORKER_HOST);
  const [workerPort, setWorkerPort] = useState(DEFAULT_WORKER_PORT);
  const [workerUrlOverride, setWorkerUrlOverride] = useState("");
  const [useWorkerUrlOverride, setUseWorkerUrlOverride] = useState(false);
  const [workerStats, setWorkerStats] = useState(null);
  const [workerCtlBusy, setWorkerCtlBusy] = useState(false);
  const [civitaiQ, setCivitaiQ] = useState(BEGINNER_CIVITAI_QUERY);
  const [civitaiItems, setCivitaiItems] = useState([]);
  const [civitaiSubview, setCivitaiSubviewState] = useState(readStoredCivitaiSubview);
  const setCivitaiSubview = (next) => {
    setCivitaiSubviewState(next);
    persistCivitaiSubview(next);
  };
  const [favorites, setFavorites] = useState(readFavorites);
  const [civitaiDetail, setCivitaiDetail] = useState(null);
  const [civitaiDetailBusy, setCivitaiDetailBusy] = useState(false);
  const [detailImageIdx, setDetailImageIdx] = useState(0);
  const [videoModels, setVideoModels] = useState([]);
  const [agentModels, setAgentModels] = useState(FALLBACK_AGENT_MODELS);
  const [imageModels, setImageModels] = useState(FALLBACK_IMAGE_MODELS);
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
  const [session, setSession] = useState(() => normalizeSession(bootSession.session));
  const [automationMode, setAutomationMode] = useState(readStoredMode);
  const storedModels = readStoredModels();
  const [agentModel, setAgentModel] = useState(storedModels.agentModel);
  const [imageModel, setImageModel] = useState(storedModels.imageModel);
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
  const [workspaceSplit, setWorkspaceSplit] = useState(readStoredWorkspaceSplit);
  const [workspaceResizing, setWorkspaceResizing] = useState(false);
  const [activeSessionId, setActiveSessionId] = useState(
    () => bootSession.id || makeSessionId(),
  );
  const [sessions, setSessions] = useState(() =>
    storedSessions?.sessions?.length ? storedSessions.sessions : [bootSession],
  );
  const sessionsRef = useRef(sessions);
  const activeIdRef = useRef(activeSessionId);
  const timelineRef = useRef(null);
  const workspaceRef = useRef(null);
  const workspaceSplitRef = useRef(workspaceSplit);
  const civitaiAbortRef = useRef(null);
  const lastCivitaiSearchQRef = useRef("");

  useEffect(() => {
    workspaceSplitRef.current = workspaceSplit;
  }, [workspaceSplit]);

  useEffect(() => {
    sessionsRef.current = sessions;
  }, [sessions]);

  useEffect(() => {
    activeIdRef.current = activeSessionId;
  }, [activeSessionId]);

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

  const snapshotCurrent = (id = activeSessionId) => {
    const prev = sessionsRef.current.find((s) => s.id === id);
    const now = new Date().toISOString();
    return {
      id,
      title: sessionTitleFrom(prompt, session),
      createdAt: prev?.createdAt || now,
      updatedAt: now,
      prompt,
      session: {
        goal: session.goal || "",
        pinnedLoras: session.pinnedLoras || [],
        turns: session.turns || [],
        messages: session.messages || [],
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
    setSession(normalizeSession(entry.session));
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
    const next = [...list];
    next[idx] = { ...snap, createdAt: list[idx].createdAt || snap.createdAt };
    return next;
  };

  const persistAll = (activeId, list) => {
    setSessions(list);
    sessionsRef.current = list;
    activeIdRef.current = activeId;
    persistSessionsStore(activeId, list);
  };

  const startNewSession = () => {
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
    const snap = snapshotCurrent();
    const list = upsertSessionList(snap);
    persistAll(id, list);
    setActiveSessionId(id);
    applySessionEntry(target);
    setTab("gallery");
  };

  const setSidebar = (open) => {
    setSidebarOpen(open);
    try {
      localStorage.setItem(SIDEBAR_STORAGE_KEY, open ? "1" : "0");
    } catch {
      /* ignore */
    }
  };

  const clampWorkspaceSplit = (clientX) => {
    const el = workspaceRef.current;
    if (!el) return workspaceSplitRef.current;
    const rect = el.getBoundingClientRect();
    const gutter = 6;
    const usable = Math.max(1, rect.width - gutter);
    const minLeftPct = (WORKSPACE_SPLIT_MIN_LEFT_PX / usable) * 100;
    const minRightPct = (WORKSPACE_SPLIT_MIN_RIGHT_PX / usable) * 100;
    const raw = ((clientX - rect.left) / usable) * 100;
    const lo = Math.min(minLeftPct, 85);
    const hi = Math.max(100 - Math.min(minRightPct, 85), lo);
    return Math.min(hi, Math.max(lo, raw));
  };

  const onWorkspaceSplitPointerDown = (e) => {
    if (e.button != null && e.button !== 0) return;
    e.preventDefault();
    const handle = e.currentTarget;
    handle.setPointerCapture?.(e.pointerId);
    setWorkspaceResizing(true);
    const pct = clampWorkspaceSplit(e.clientX);
    workspaceSplitRef.current = pct;
    setWorkspaceSplit(pct);

    const onMove = (ev) => {
      const next = clampWorkspaceSplit(ev.clientX);
      workspaceSplitRef.current = next;
      setWorkspaceSplit(next);
    };
    const onUp = (ev) => {
      handle.releasePointerCapture?.(ev.pointerId);
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
      setWorkspaceResizing(false);
      persistWorkspaceSplit(workspaceSplitRef.current);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
  };

  const onWorkspaceSplitKeyDown = (e) => {
    const step = e.shiftKey ? 5 : 2;
    let next = workspaceSplitRef.current;
    if (e.key === "ArrowLeft") next -= step;
    else if (e.key === "ArrowRight") next += step;
    else if (e.key === "Home") next = 20;
    else if (e.key === "End") next = 80;
    else return;
    e.preventDefault();
    const el = workspaceRef.current;
    if (el) {
      const rect = el.getBoundingClientRect();
      const gutter = 6;
      const usable = Math.max(1, rect.width - gutter);
      const minLeftPct = (WORKSPACE_SPLIT_MIN_LEFT_PX / usable) * 100;
      const minRightPct = (WORKSPACE_SPLIT_MIN_RIGHT_PX / usable) * 100;
      next = Math.min(100 - Math.min(minRightPct, 85), Math.max(Math.min(minLeftPct, 85), next));
    } else {
      next = Math.min(80, Math.max(20, next));
    }
    workspaceSplitRef.current = next;
    setWorkspaceSplit(next);
    persistWorkspaceSplit(next);
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

  const applyModelCatalog = (models) => {
    setVideoModels(Array.isArray(models?.video) ? models.video : []);
    setAgentModels(
      catalogToOptions(models?.chat, {
        auto: AUTO_AGENT,
        fallback: FALLBACK_AGENT_MODELS,
      }),
    );
    setImageModels(
      catalogToOptions(models?.openrouter, {
        auto: AUTO_IMAGE,
        fallback: FALLBACK_IMAGE_MODELS,
      }),
    );
  };

  const refresh = async () => {
    // Studio payload and OpenRouter catalogs are independent: a 401/failure on
    // styles/gallery must not skip model discovery once a bearer is present.
    try {
      const [s, st, g] = await Promise.all([
        api("/v1/styles"),
        api("/v1/settings"),
        api("/v1/gallery"),
      ]);
      const items = s.items || [];
      setStyles(items);
      setSettings(st);
      {
        const ep = parseWorkerEndpoint(st.diffusionWorkerUrl || "");
        setWorkerHost(ep.host);
        setWorkerPort(ep.port);
        setWorkerUrlOverride(ep.advancedUrl);
        setUseWorkerUrlOverride(ep.useAdvanced);
      }
      setGallery(g.items || []);
      const featured = items[0]?.examplePrompt;
      if (featured) {
        setCivitaiQ((prev) =>
          !prev.trim() || prev === BEGINNER_CIVITAI_QUERY ? demoCivitaiQuery(featured) : prev,
        );
      }
      setError("");
    } catch (e) {
      setError(e.message);
    }
    try {
      applyModelCatalog(await api("/v1/models"));
    } catch {
      /* keep prior / offline FALLBACK_* lists until auth or OpenRouter works */
    }
  };

  const isFavorite = (item) =>
    favorites.some((f) => loraKey(f) === loraKey(item));

  const toggleFavorite = (item) => {
    setFavorites((prev) => {
      const key = loraKey(item);
      const exists = prev.some((f) => loraKey(f) === key);
      const next = exists
        ? prev.filter((f) => loraKey(f) !== key)
        : [
            {
              id: item.id,
              name: item.name,
              modelType: item.modelType,
              downloadUrl: item.downloadUrl,
              thumbnailUrl: item.thumbnailUrl,
              triggerWord: item.triggerWord || item.trainedWords?.[0],
              baseModel: item.baseModel,
              civitaiUrl: item.civitaiUrl || (item.id ? `https://civitai.com/models/${item.id}` : null),
              trainedWords: item.trainedWords,
            },
            ...prev,
          ].slice(0, 200);
      writeFavorites(next);
      return next;
    });
  };

  const closeCivitaiDetail = () => {
    setCivitaiDetail(null);
    setDetailImageIdx(0);
  };

  const openCivitaiDetail = async (item) => {
    if (!item?.id && !item?.name) return;
    setCivitaiDetailBusy(true);
    setDetailImageIdx(0);
    setCivitaiDetail({ model: item, related: [], loading: true });
    try {
      if (item.id != null && settings?.civitaiConfigured) {
        const data = await api(`/v1/civitai/models/${item.id}`);
        // Accept nested { model, related } or a flat detail payload.
        const model = data.model || data;
        const related = Array.isArray(data.related)
          ? data.related
          : Array.isArray(model.related)
            ? model.related
            : [];
        setCivitaiDetail({
          model: { ...item, ...model },
          related,
          loading: false,
        });
      } else {
        setCivitaiDetail({ model: item, related: item.related || [], loading: false });
      }
    } catch (e) {
      // Keep the list/suggest card usable if detail fetch misses (e.g. mock catalog vs live id).
      setCivitaiDetail({
        model: item,
        related: [],
        loading: false,
        error: item?.name ? null : e.message,
      });
    } finally {
      setCivitaiDetailBusy(false);
    }
  };

  useEffect(() => {
    refresh();
  }, []);

  const refreshWorkerStats = async () => {
    try {
      const st = await api("/v1/worker/stats");
      setWorkerStats(st);
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
  }, [tab, mock]);

  const workerControl = async (action) => {
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
      setSavedMsg(
        action === "stop"
          ? `Worker stop requested${health ? ` · health diffusionWorker=${health.diffusionWorker}` : ""}`
          : action === "start"
            ? `Worker start requested${health ? ` · health diffusionWorker=${health.diffusionWorker}` : ""}`
            : action === "disable"
              ? "Worker disabled — local gens skipped; OpenRouter still works"
              : "Worker enabled",
      );
    } catch (e) {
      setError(e.message);
    } finally {
      setWorkerCtlBusy(false);
    }
  };

  const persistModels = (nextAgent, nextImage) => {
    try {
      localStorage.setItem(
        MODELS_STORAGE_KEY,
        JSON.stringify({ agentModel: nextAgent, imageModel: nextImage }),
      );
    } catch {
      /* ignore */
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
  };

  const changeAgentModel = (id) => {
    setAgentModel(id);
    persistModels(id, imageModel);
  };

  const changeImageModel = (id) => {
    setImageModel(id);
    persistModels(agentModel, id);
  };

  const composedWorkerUrl = () =>
    useWorkerUrlOverride
      ? workerUrlOverride.trim()
      : composeWorkerUrl(workerHost, workerPort);
  // Prefer persisted URL so default placeholders do not fake a configured worker.
  const hasWorker = () => Boolean(settings?.diffusionWorkerUrl);
  const canAutoInstall = () => Boolean(settings?.civitaiConfigured && hasWorker());

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
      setSavedMsg(
        res.rotated
          ? "API token rotated — copy it now; the previous bearer no longer works. Model lists refreshed."
          : "API token generated — copy it now; it will not be shown again. Model lists refreshed.",
      );
    } catch (e) {
      setError(
        e.message === "Unauthorized" || /401|Unauthorized/i.test(e.message || "")
          ? "Generate needs the current bearer (paste under Advanced), or clear FREYA_API_TOKEN for open local/dev."
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

  /** Persist pasted bearer and reload OpenRouter Agent/Image catalogs immediately. */
  const applyStudioToken = async () => {
    persistStudioTokenLocal();
    setError("");
    setSavedMsg("");
    await refresh();
    setSavedMsg(
      studioToken.trim()
        ? "Studio token saved — Agent/Image model lists refreshed from /v1/models."
        : "Studio token cleared.",
    );
  };

  const saveWorkerEndpoint = async () => {
    setBusy(true);
    setError("");
    setSavedMsg("");
    try {
      persistStudioTokenLocal();
      const st = await api("/v1/settings", {
        method: "PUT",
        body: JSON.stringify({ diffusionWorkerUrl: composedWorkerUrl() }),
      });
      setSettings(st);
      applyWorkerEndpoint(st.diffusionWorkerUrl || "");
      await refreshWorkerStats();
      setSavedMsg(
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
      name: l.path
        ? String(l.path).split("/").pop().replace(/\.safetensors$/i, "")
        : l.name,
      scale: l.scale ?? 0.8,
      path: l.path || undefined,
      baseModel: l.baseModel || undefined,
    }));

  const isPinned = (item) =>
    (session.pinnedLoras || []).some((p) => loraKey(p) === loraKey(item));

  const pinLora = (item, extra = {}) => {
    setSession((prev) => {
      if ((prev.pinnedLoras || []).some((p) => loraKey(p) === loraKey(item))) return prev;
      const next = {
        id: item.id,
        name: item.name,
        modelType: item.modelType,
        downloadUrl: item.downloadUrl,
        triggerWord: item.triggerWord || null,
        baseModel: item.baseModel || null,
        path: extra.path || item.path || null,
        versionId: item.versionId || null,
        scale: 0.8,
      };
      const baseHint = item.baseModel
        ? ` Needs base family “${item.baseModel}” on the local worker (Worker → active base / install checkpoint).`
        : "";
      return {
        ...prev,
        pinnedLoras: [...(prev.pinnedLoras || []), next],
        sessionNote: item.triggerWord
          ? `Pinned “${item.name}”. Trigger word “${item.triggerWord}” is optional — Freya keeps your prompt as you wrote it.${baseHint}`
          : `Pinned “${item.name}” for this session. Your prompt stays as you wrote it.${baseHint}`,
      };
    });
    setSavedMsg(
      item.baseModel
        ? `Pinned ${item.name} (base: ${item.baseModel}) for this session.`
        : `Pinned ${item.name} for this session.`,
    );
  };

  const unpinLora = (item) => {
    setSession((prev) => ({
      ...prev,
      pinnedLoras: (prev.pinnedLoras || []).filter((p) => loraKey(p) !== loraKey(item)),
    }));
  };

  const pushTurn = (text, artifactIds = []) => {
    const at = new Date().toISOString();
    setSession((prev) => ({
      ...prev,
      goal: prev.goal || text,
      turns: [{ prompt: text, at, artifactIds }, ...(prev.turns || [])].slice(0, 12),
      messages: [
        ...(prev.messages || []),
        { role: "user", content: text, at },
        ...(artifactIds?.length
          ? [
              {
                role: "assistant",
                content: `Generated ${artifactIds.length} image${artifactIds.length === 1 ? "" : "s"}.`,
                at,
                artifactIds,
              },
            ]
          : []),
      ].slice(-40),
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
    const lorasNext = Array.isArray(data.loras) ? data.loras : [];
    const query = data.civitaiQuery || "";

    if (ideasNext.length) setIdeas(ideasNext);
    if (lorasNext.length) setSuggestedLoras(lorasNext);
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
        if (a.type === "search_civitai")
          return `Searched Civitai for “${a.result?.query || a.args?.query || "…"}”`;
        if (a.type === "suggest_ideas")
          return `Suggested ${(a.result?.ideas || []).length || "some"} ideas`;
        if (a.type === "install_asset") return `Installed “${a.args?.name || "asset"}”`;
        return `${a.type}: ${a.status}`;
      });
    if (toolLines.length) setSessionLog(toolLines);

    const at = new Date().toISOString();
    appendMessages({
      role: "assistant",
      content: data.reply || (artifacts.length ? "Done." : "…"),
      at,
      actions,
      artifactIds,
      ideas: ideasNext,
      loras: lorasNext,
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
    if (agentModel && agentModel !== "openrouter/auto") body.model = agentModel;
    if (imageModel && imageModel !== "auto") body.imageModel = imageModel;
    return body;
  };

  /** Primary composer action — send a chat message; tools run per Plan/Auto/YOLO. */
  const submitChat = async (overrideText, extra = {}) => {
    const text = (overrideText ?? prompt).trim();
    if (!text && !extra.confirm) return;
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
    const t = (item.modelType || "").toLowerCase();
    if (t === "lora" || t === "locon" || t === "dora" || t === "lycoris" || t.includes("lora")) {
      return `loras/${safe}.safetensors`;
    }
    if (t === "textualinversion") return `embeddings/${safe}.safetensors`;
    if (t === "vae") return `vae/${safe}.safetensors`;
    return `checkpoints/${safe}.safetensors`;
  };

  const installOne = async (item) => {
    const dest = destPathForAsset(item);
    await api("/v1/civitai/install", {
      method: "POST",
      body: JSON.stringify({
        downloadUrl: item.downloadUrl,
        destPath: dest,
        name: item.name,
        baseModel: item.baseModel || undefined,
        versionId: item.versionId || undefined,
        modelType: item.modelType || undefined,
      }),
    });
    pinLora(item, { path: dest });
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

  /** Shortcut: inject a suggest request into chat (tool: suggest_ideas). */
  const suggestIdeas = async () => {
    const topic = prompt.trim() || session.goal || examplePrompt;
    const msg = `Suggest ideas for: ${topic}`;
    setPrompt(msg);
    await submitChat(msg);
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
      log.push(`Agent model: ${modelLabel(agentModels, agentModel)}`);
    }
    if (imageModel && imageModel !== "auto") {
      log.push(`Image model: ${modelLabel(imageModels, imageModel)}`);
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

  /** Shortcut: inject a run-session request into chat. */
  const runSession = async () => {
    const goal = prompt.trim() || session.goal || examplePrompt;
    const msg = `Run session for: ${goal}`;
    setPrompt(msg);
    await submitChat(msg);
  };

  const confirmPendingActions = async () => {
    if (!pendingPlan?.actions?.length) return;
    await submitChat(pendingPlan.goal || prompt || examplePrompt, {
      confirm: true,
      actions: pendingPlan.actions.map((a) => ({
        type: a.type,
        args: a.args || {},
      })),
    });
    setPendingPlan(null);
    setPendingQuestion(null);
    setSavedMsg("Confirmed — Freya ran the proposed tools.");
  };

  const answerQuestion = async (choice) => {
    if (!pendingPlan || !pendingQuestion) return;

    if (choice.value === "settings") {
      setTab("settings");
      setSavedMsg(
        "Add a worker URL (and Civitai key) for LoRA installs — OpenRouter still generates without one.",
      );
      return;
    }

    if (choice.value === "skip" || choice.id === "skip") {
      dismissPending();
      appendMessages({
        role: "assistant",
        content: "Skipped proposed tools — keep chatting whenever you’re ready.",
        at: new Date().toISOString(),
      });
      return;
    }

    if (choice.value === "confirm" || choice.id === "confirm" || choice.id === "all") {
      await confirmPendingActions();
      return;
    }

    // Single LoRA choice — confirm only that install (+ any generate)
    const actions = (pendingPlan.actions || []).filter((a) => {
      if (a.type !== "install_asset") return true;
      if (choice.value && typeof choice.value === "object") {
        return a.args?.name === choice.value.name || a.args?.downloadUrl === choice.value.downloadUrl;
      }
      return false;
    });
    await submitChat(pendingPlan.goal || prompt || examplePrompt, {
      confirm: true,
      actions: actions.map((a) => ({ type: a.type, args: a.args || {} })),
    });
    setPendingPlan(null);
    setPendingQuestion(null);
  };

  const dismissPending = () => {
    setPendingPlan(null);
    setPendingQuestion(null);
    setSavedMsg("Dismissed.");
  };

  const saveSettings = async () => {
    setBusy(true);
    setError("");
    setSavedMsg("");
    try {
      // Persist studio bearer first so the PUT below authenticates when FREYA_API_TOKEN is set.
      persistStudioTokenLocal();
      const body = {};
      if (civitaiKey.trim()) body.civitaiApiKey = civitaiKey.trim();
      if (openrouterKey.trim()) body.openrouterApiKey = openrouterKey.trim();
      const st = await api("/v1/settings", {
        method: "PUT",
        body: JSON.stringify(body),
      });
      setSettings(st);
      setCivitaiKey("");
      setOpenrouterKey("");
      // Bearer / OpenRouter key may have just been written — refresh catalogs.
      await refresh();
      setSavedMsg(
        mock
          ? "Mock settings saved in-memory (reload resets unless you keep this tab open)."
          : "Settings saved. Provider keys stay on the API; studio bearer stays in this browser. Model lists refreshed.",
      );
    } catch (e) {
      setError(e.message);
    } finally {
      setBusy(false);
    }
  };

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
      // Default Freya types on the API; pass explicitly so mock + live stay aligned.
      const data = await api(
        `/v1/civitai/search?q=${encodeURIComponent(q)}&limit=12&types=${encodeURIComponent(CIVITAI_SEARCH_TYPES)}`,
        { signal: ac.signal },
      );
      if (ac.signal.aborted) return;
      setCivitaiItems(data.items || []);
      lastCivitaiSearchQRef.current = q;
    } catch (e) {
      if (e?.name === "AbortError" || ac.signal.aborted) return;
      setError(e.message);
    } finally {
      if (civitaiAbortRef.current === ac) {
        civitaiAbortRef.current = null;
        setBusy(false);
      }
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

  const conversation = session.messages || [];

  // Keep the newest history in view above the pinned composer.
  useEffect(() => {
    const el = timelineRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [
    activeSessionId,
    conversation.length,
    sessionLog.length,
    ideas.length,
    pendingPlan,
    pendingQuestion,
  ]);

  const hasTimeline =
    conversation.length > 0 ||
    Boolean(session.goal) ||
    (session.pinnedLoras || []).length > 0 ||
    Boolean(session.sessionNote);

  const renderLoraCard = (item, { compact = false, pinLabel, onAfterPin } = {}) => {
    const pinned = isPinned(item);
    const favorited = isFavorite(item);
    const typeLabel = item.modelType || "LORA";
    const downloads = formatStat(item.stats?.downloadCount);
    return (
      <article key={loraKey(item)} className={`lora-card${compact ? " compact" : ""}`}>
        <button
          type="button"
          className="lora-card-thumb lora-card-thumb-btn"
          onClick={() => openCivitaiDetail(item)}
          title="View details"
        >
          {item.thumbnailUrl ? (
            <img
              src={item.thumbnailUrl}
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
          <button type="button" className="lora-card-title" onClick={() => openCivitaiDetail(item)}>
            <h4>{item.name}</h4>
          </button>
          {item.baseModel ? <p className="lora-card-meta">{item.baseModel}</p> : null}
          {item.triggerWord ? <p>trigger: {item.triggerWord}</p> : null}
        </div>
        <div className="row lora-card-actions">
          <button
            type="button"
            className={favorited ? "primary" : "ghost"}
            title={favorited ? "Remove from favorites" : "Save to favorites library"}
            onClick={() => toggleFavorite(item)}
          >
            {favorited ? "★" : "☆"}
          </button>
          <button
            type="button"
            className={pinned ? "primary" : "ghost"}
            title={pinned ? "Unpin from this session" : "Pin for this session only"}
            onClick={() => {
              if (pinned) unpinLora(item);
              else {
                pinLora(item);
                onAfterPin?.(item);
              }
            }}
          >
            {pinned ? "Unpin" : pinLabel || "Pin"}
          </button>
          <button
            type="button"
            className="ghost"
            disabled={busy}
            title={
              hasWorker()
                ? "Install onto your diffusion worker"
                : "Needs a worker URL — pin for later, or open the Worker tab"
            }
            onClick={() => installCivitai(item)}
          >
            {hasWorker() ? "Install" : "Install (needs worker)"}
          </button>
        </div>
      </article>
    );
  };

  const detailModel = civitaiDetail?.model;
  const detailImages =
    (detailModel?.imageUrls?.length
      ? detailModel.imageUrls
      : detailModel?.thumbnailUrl
        ? [detailModel.thumbnailUrl]
        : []) || [];
  const detailRelated = civitaiDetail?.related || [];

  const showAssistPanel =
    ideas.length > 0 || suggestedLoras.length > 0 || suggestNote || civitaiQuery;

  const modeOptions = AUTOMATION_MODES.map((id) => ({
    id,
    label: AUTOMATION_LABELS[id],
  }));
  const agentOptions = ensureSelectedOption(agentModels, agentModel);
  const imageOptions = ensureSelectedOption(imageModels, imageModel);

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
                    workerStats?.reachable && settings.workerEnabled !== false
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
            {sessions.map((s) => {
              const active = s.id === activeSessionId;
              const imgs = sessionImageCount(s);
              const title = active
                ? sessionTitleFrom(prompt, session)
                : s.title || sessionTitleFrom(s.prompt, s.session);
              return (
                <li key={s.id}>
                  <button
                    type="button"
                    role="option"
                    aria-selected={active}
                    className={`session-item${active ? " active" : ""}`}
                    onClick={() => switchSession(s.id)}
                  >
                    <span className="session-item-title">{title}</span>
                    <span className="session-item-meta">
                      <time dateTime={s.updatedAt || s.createdAt}>
                        {relativeTime(s.updatedAt || s.createdAt)}
                      </time>
                      {imgs > 0 && (
                        <span>
                          {imgs} img{imgs === 1 ? "" : "s"}
                        </span>
                      )}
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
        </aside>

      <div
        className={`workspace${workspaceResizing ? " is-resizing" : ""}`}
        ref={workspaceRef}
        style={{ "--workspace-split": `${workspaceSplit}%` }}
      >
        <section className="pane pane-left" aria-label="Prompt and session">
          <div className="timeline" aria-label="Conversation" ref={timelineRef}>
            <div className="timeline-head">
              <h2>Conversation</h2>
              <span className="timeline-count">
                {conversation.length
                  ? `${conversation.length} message${conversation.length === 1 ? "" : "s"}`
                  : "no messages yet"}
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
                Chat with Freya here. Ask about style, or say “make an image of …” when you want a
                generate. Suggest and Session shortcuts just send a message.
              </p>
            )}

            {(session.pinnedLoras || []).length > 0 && (
              <article className="t-entry t-pins">
                <span className="t-kind">Pinned LoRAs</span>
                <ul className="pin-list">
                  {(session.pinnedLoras || []).map((l) => (
                    <li key={loraKey(l)}>
                      <span>
                        {l.name}
                        {l.baseModel ? ` · base ${l.baseModel}` : ""}
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

            {conversation.map((m, i) => (
              <article
                key={`${m.at || i}-${m.role}-${i}`}
                className={`t-entry t-msg t-${m.role === "user" ? "user" : "assistant"}`}
              >
                <div className="t-meta">
                  <span className="t-kind">{m.role === "user" ? "You" : "Freya"}</span>
                  {m.at && (
                    <time>
                      {new Date(m.at).toLocaleTimeString([], {
                        hour: "numeric",
                        minute: "2-digit",
                      })}
                    </time>
                  )}
                </div>
                <p className="t-bubble">{m.content}</p>
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
                      return (
                        <button
                          key={id}
                          type="button"
                          className="t-thumb"
                          onClick={() => {
                            if (art) {
                              setResult(art.artifactId ? art : { ...art, artifactId: art.id, url: gallerySrc(art) });
                              setTab("gallery");
                            }
                          }}
                        >
                          <img src={src} alt="" />
                        </button>
                      );
                    })}
                  </div>
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

            {pendingPlan && (pendingPlan.status === "plan" || pendingPlan.actions?.length) && (
              <aside className="pending-plan" aria-label="Proposed tools">
                <div className="pending-plan-head">
                  <h3>Proposed tools</h3>
                  <button type="button" className="ghost" onClick={dismissPending}>
                    Dismiss
                  </button>
                </div>
                {pendingPlan.goal && (
                  <p className="pending-goal">
                    <span className="session-label">For</span>
                    {pendingPlan.goal}
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
                  </div>
                )}
                <p className="hint">
                  Plan mode proposes generate/install — confirm to run, or switch to Auto/YOLO.
                </p>
                {pendingPlan.actions?.length > 0 && (
                  <div className="pending-ask-choices">
                    <button
                      type="button"
                      className="primary"
                      disabled={busy}
                      onClick={confirmPendingActions}
                    >
                      Confirm & run
                    </button>
                    <button type="button" className="ghost" disabled={busy} onClick={dismissPending}>
                      Not now
                    </button>
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
                {suggestedLoras.length > 0 && (
                  <p className="hint">
                    {suggestedLoras.length} Civitai{" "}
                    {suggestedLoras.length === 1 ? "asset" : "assets"} in the gallery pane → pin or
                    install there.
                  </p>
                )}
              </aside>
            )}
          </div>

          <div className="compose">
            <div className="compose-head">
              <label className="compose-label" htmlFor="freya-prompt">
                Message Freya
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
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  if (!busy && prompt.trim()) submitChat();
                }
              }}
              placeholder={examplePrompt}
              rows={3}
              title="Chat with Freya — Enter to send, Shift+Enter for a new line"
              aria-describedby="freya-prompt-hint"
            />
            <p id="freya-prompt-hint" className="sr-only">
              Chat with Freya. Ask to generate only when you want an image. Mode gates tool
              autonomy. {MODE_HINTS[automationMode]}
            </p>
            {error && <p className="err compose-status">{error}</p>}
            {savedMsg && (
              <p className="hint saved-msg compose-status" role="status">
                {savedMsg}
              </p>
            )}
            <div className="composer-bar" aria-label="Chat controls">
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
                <button
                  type="button"
                  className="ghost icon-btn"
                  disabled={busy || (!prompt.trim() && !session.goal)}
                  onClick={runSession}
                  title={`Ask Freya to run a session (${AUTOMATION_LABELS[automationMode]})`}
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
                  title={`Agent model: ${modelLabel(agentOptions, agentModel)}`}
                  valueLabel={modelShortLabel(agentOptions, agentModel)}
                  options={agentOptions}
                  value={agentModel}
                  onChange={changeAgentModel}
                  open={menuOpen === "agent"}
                  onOpen={(v) => setMenuOpen(v ? "agent" : null)}
                />
                <CompactMenu
                  label="Image"
                  icon={<IconImage />}
                  title={`Image model: ${modelLabel(imageOptions, imageModel)}`}
                  valueLabel={modelShortLabel(imageOptions, imageModel)}
                  options={imageOptions}
                  value={imageModel}
                  onChange={changeImageModel}
                  open={menuOpen === "image"}
                  onOpen={(v) => setMenuOpen(v ? "image" : null)}
                />
              </div>
              <div className="composer-primary-cluster" role="group" aria-label="Suggest and send">
                <button
                  type="button"
                  className="ghost icon-btn"
                  disabled={busy}
                  onClick={suggestIdeas}
                  title="Suggest ideas (sends a chat message)"
                  aria-label="Suggest ideas"
                >
                  <IconBulb />
                  <span className="sr-only">Suggest ideas</span>
                </button>
                <button
                  type="button"
                  className="primary icon-btn icon-btn-primary composer-send"
                  disabled={busy || !prompt.trim()}
                  onClick={() => submitChat()}
                  title={busy ? "Working…" : "Send message"}
                  aria-label={busy ? "Working" : "Send message"}
                >
                  {busy ? <IconSpinner /> : <IconSend />}
                  <span className="sr-only">{busy ? "Working…" : "Send"}</span>
                </button>
              </div>
            </div>
          </div>
        </section>

        <button
          type="button"
          role="separator"
          className="workspace-split"
          aria-orientation="vertical"
          aria-label="Resize chat and gallery panes"
          aria-valuemin={15}
          aria-valuemax={85}
          aria-valuenow={Math.round(workspaceSplit)}
          title="Drag to resize"
          onPointerDown={onWorkspaceSplitPointerDown}
          onKeyDown={onWorkspaceSplitKeyDown}
        />

        <section className="pane pane-right" aria-label="Gallery and results">
          {suggestedLoras.length > 0 && (
            <aside className="lora-strip" aria-label="Civitai asset suggestions">
              <div className="lora-strip-head">
                <div>
                  <span className="t-kind">Civitai assets</span>
                  {civitaiQuery ? (
                    <p className="lora-strip-q">
                      Civitai · <strong>{civitaiQuery}</strong>
                    </p>
                  ) : (
                    <p className="lora-strip-q">Suggested for this prompt</p>
                  )}
                </div>
                <button type="button" className="ghost" onClick={clearSuggestions}>
                  Dismiss
                </button>
              </div>
              <p className="hint">
                LoRAs ranked first; checkpoints and other weights included. Pin anytime (no worker
                needed). Install needs a worker — otherwise Freya generates via OpenRouter.
              </p>
              <div className="lora-row lora-row-strip">
                {suggestedLoras.map((l) => renderLoraCard(l, { compact: true }))}
              </div>
            </aside>
          )}

      {tab === "gallery" && !result && gallery.length > 0 && (
        <section className="salon salon-single">
          <div className="salon-head">
            <span className="t-kind">Latest in gallery</span>
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
          <p>No images yet — ask Freya to make an image and results will hang here.</p>
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
        {STUDIO_TABS.map((t) => (
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
          <div className="row civitai-subnav" style={{ marginBottom: "0.75rem" }}>
            <button
              type="button"
              className={civitaiSubview === "search" ? "primary" : "ghost"}
              onClick={() => setCivitaiSubview("search")}
            >
              Search
            </button>
            <button
              type="button"
              className={civitaiSubview === "favorites" ? "primary" : "ghost"}
              onClick={() => setCivitaiSubview("favorites")}
            >
              Favorites ({favorites.length})
            </button>
          </div>
          {civitaiSubview === "search" && (
            <>
              {!settings?.civitaiConfigured && (
                <p className="hint">
                  Add your Civitai API key in Settings to search assets (LoRA, Checkpoint, and
                  related weights)
                  {mock ? " — any non-empty value works in mock" : ""}. ★ Favorite is a persistent
                  library; Pin is session-only. Install needs a worker.
                </p>
              )}
              {settings?.civitaiConfigured && !hasWorker() && (
                <p className="hint">
                  Worker not set — search, favorite, and pin work now. Install needs a diffusion
                  worker URL; Generate still works via OpenRouter without one.
                </p>
              )}
              <p className="hint civitai-demo-hint">
                Featured example: describe something like “{examplePrompt}” — Freya searches for “
                {exampleCivitaiQuery}”. Click a card for details, triggers, and related checkpoints.
              </p>
              <div className="row" style={{ marginBottom: "1rem" }}>
                <input
                  style={{ flex: 1, minWidth: "12rem" }}
                  value={civitaiQ}
                  onChange={(e) => setCivitaiQ(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      e.preventDefault();
                      searchCivitai({ immediate: true });
                    }
                  }}
                  placeholder="Search Civitai…"
                />
                <button
                  className="primary"
                  disabled={busy}
                  onClick={() => searchCivitai({ immediate: true })}
                >
                  Search
                </button>
              </div>
              {savedMsg && <p className="hint">{savedMsg}</p>}
              <div className="lora-row">
                {civitaiItems.map((item) =>
                  renderLoraCard(item, {
                    pinLabel: "Pin to session",
                    onAfterPin: () => setTab("gallery"),
                  }),
                )}
              </div>
            </>
          )}
          {civitaiSubview === "favorites" && (
            <>
              <p className="hint">
                Persistent library in this browser (localStorage). ★ Favorite survives sessions; Pin
                only applies to the current session generate. Install/Pin still available here.
              </p>
              {favorites.length === 0 ? (
                <p className="hint">No favorites yet — star assets from Search or Suggest.</p>
              ) : (
                <div className="lora-row">
                  {favorites.map((item) =>
                    renderLoraCard(item, {
                      pinLabel: "Pin to session",
                      onAfterPin: () => setTab("gallery"),
                    }),
                  )}
                </div>
              )}
            </>
          )}
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
          <p className="hint">
            Local LoRAs and custom looks need a diffusion worker. Cloud Generate via OpenRouter
            works without one. Provider API keys live under{" "}
            <button type="button" className="linkish" onClick={() => setTab("settings")}>
              Settings
            </button>
            .
          </p>

          <div className="worker-layout">
            <aside className="worker-setup-panel" aria-label="Setup and connect">
              <div className="settings-block-head">
                <strong>Setup</strong>
                <span className="hint">how to point Studio at a worker</span>
              </div>

              <p className="hint">
                Run a worker on this machine with <code>make run-worker</code> or Docker{" "}
                <code>freya-local-worker</code> (binds <code>0.0.0.0:8091</code>). Models default
                to <code>/mnt/game2/freya/models</code> (<code>FREYA_MODELS_DIR</code>). In kind,
                Studio is <code>freya.dataknife.net</code>; the sample worker CR is{" "}
                <code>FreyaDiffusionWorker/freya-gpu</code> in namespace <code>freya</code>.
              </p>

              <div className="worker-connect" aria-label="Connect options">
                <strong className="worker-connect-title">Connect</strong>
                <p className="hint">
                  Put the worker address in Host / Port (or Advanced full URL), then Save worker.
                  Pick the form that matches where Freya API can reach the process:
                </p>
                <ul className="worker-setup-steps">
                  <li>
                    <strong>Localhost</strong> — API and worker on the same host: Host{" "}
                    <code>{DEFAULT_WORKER_HOST}</code> (or <code>127.0.0.1</code>), Port{" "}
                    <code>{DEFAULT_WORKER_PORT}</code>. Use <em>Use local defaults</em>.
                  </li>
                  <li>
                    <strong>Internal Kubernetes DNS</strong> — in-cluster API talking to an
                    in-cluster worker Service:{" "}
                    <code>freya-gpu.freya.svc.cluster.local</code> on port <code>8091</code>{" "}
                    (operator pattern <code>{"{name}.{namespace}.svc.cluster.local"}</code>;
                    sample name <code>freya-gpu</code>, namespace <code>freya</code>). Kind-local
                    ExternalEndpoint example:{" "}
                    <code>http://freya-gpu-host.freya.svc.cluster.local:8091</code>.
                  </li>
                  <li>
                    <strong>External FQDN</strong> — Ingress / public endpoint:{" "}
                    <code>https://vllm.dataknife.net</code> (sample{" "}
                    <code>externalEndpoint</code>; health at{" "}
                    <code>/health</code>). Prefer Advanced → full worker URL for https.
                  </li>
                  <li>
                    <strong>Host IP + port</strong> — GPU box elsewhere on the LAN: put that
                    machine&apos;s IP in Host and the worker listen port (default{" "}
                    <code>8091</code>) in Port — same surface as{" "}
                    <code>http://&lt;ip&gt;:8091</code>.
                  </li>
                </ul>
                <p className="hint">
                  SD/SDXL/Pony/Flux bases belong on the local GPU worker (RTX /{" "}
                  <code>freya-local-worker</code>). Cluster <code>vllm.dataknife.net</code> is
                  often text-only — do not treat it as a drop-in for local diffusion weights.
                </p>
              </div>
            </aside>

            <div className="worker-controls-panel" aria-label="Worker controls">
              <div className="settings-block-head">
                <strong>Controls</strong>
                <span className="hint">endpoint, auth, runtime</span>
              </div>

              <div className="settings-block worker-auth-block" aria-label="Auth">
                <div className="settings-block-head">
                  <strong>Auth</strong>
                  <span className="hint">
                    {settings?.apiTokenConfigured
                      ? "API auth enabled"
                      : "API open (no token) — local/dev"}
                  </span>
                </div>
                <div className="row settings-token-actions">
                  <button
                    type="button"
                    className="primary"
                    disabled={busy || tokenBusy || mock}
                    onClick={generateApiToken}
                    title="Create or rotate FREYA_API_TOKEN on the API"
                  >
                    {tokenBusy ? "Generating…" : "Generate API token"}
                  </button>
                </div>
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
                    <p className="hint">
                      Treat like a password. Stored in this browser for Studio requests. Optional
                      cluster sync: update secret <code>freya-api-token</code> then restart{" "}
                      <code>freya-api</code> if the secrets file is not on durable storage.
                    </p>
                  </div>
                )}
                {!generatedToken && studioToken && (
                  <p className="hint">
                    Bearer present in this browser
                    {settings?.apiTokenConfigured ? " · matches API auth" : ""}. Generate again to
                    rotate.
                  </p>
                )}
              </div>

              <div className="worker-panel" aria-label="Diffusion worker status">
                <div className="worker-panel-head">
                  <span
                    className={`status-dot ${
                      workerStats?.reachable &&
                      (workerStats?.enabled ?? settings?.workerEnabled) !== false
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
                            workerStats.enabled === false ? "disabled" : "enabled",
                            workerStats.reachable ? "up" : "down",
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
                  Endpoint:{" "}
                  <code>{composedWorkerUrl() || "(empty — OpenRouter only)"}</code>
                  . Clear host to disable local worker URL on Save worker.
                </p>

                <button className="primary" disabled={busy} onClick={saveWorkerEndpoint}>
                  Save worker
                </button>
                {savedMsg && <p className="hint">{savedMsg}</p>}

                {(() => {
                  const vram = workerStats?.vram;
                  const used = vram?.usedMiB;
                  const total = vram?.totalMiB;
                  const pct =
                    typeof used === "number" && typeof total === "number" && total > 0
                      ? Math.min(100, Math.round((used / total) * 100))
                      : null;
                  const util = vram?.utilPercent;
                  return (
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
                          {workerStats.modelLoaded.loadedInVram === false
                            ? " — selected on disk; GPU load needs real engine"
                            : ""}
                        </p>
                      )}
                      {workerStats?.modelsDir && (
                        <p className="hint">Models dir: {workerStats.modelsDir}</p>
                      )}
                    </div>
                  );
                })()}
                <WorkerBaseModelPanel
                  busy={busy || workerCtlBusy || mock}
                  hasWorker={hasWorker()}
                  api={api}
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
                  <button
                    type="button"
                    className="ghost"
                    disabled={busy || workerCtlBusy || mock}
                    onClick={() => workerControl("enable")}
                    title="Use configured worker for local gens"
                  >
                    Enable
                  </button>
                  <button
                    type="button"
                    className="ghost"
                    disabled={busy || workerCtlBusy || mock}
                    onClick={() => workerControl("disable")}
                    title="Detach worker — OpenRouter cloud path still works"
                  >
                    Disable
                  </button>
                </div>
              </div>

              <details className="settings-advanced">
                <summary>Advanced — paste token / full worker URL</summary>
                <label>
                  Studio API token (paste existing)
                  <input
                    type="password"
                    autoComplete="off"
                    placeholder="Bearer for /v1 when Freya API auth is enabled"
                    value={studioToken}
                    onChange={(e) => setStudioToken(e.target.value)}
                  />
                  <span className="hint">
                    Stored only in this browser (<code>localStorage.freya.apiToken</code>). Prefer{" "}
                    Generate above. Paste here to unlock Generate when the API already has a
                    token.
                  </span>
                </label>
                <div className="row settings-token-actions">
                  <button
                    type="button"
                    className="ghost"
                    disabled={busy || tokenBusy || mock}
                    onClick={applyStudioToken}
                    title="Save bearer to this browser and reload Agent/Image model lists"
                  >
                    Apply token &amp; refresh models
                  </button>
                </div>
                <label className="settings-check">
                  <input
                    type="checkbox"
                    checked={useWorkerUrlOverride}
                    onChange={(e) => setUseWorkerUrlOverride(e.target.checked)}
                  />
                  Use full worker URL (https / remote)
                </label>
                {useWorkerUrlOverride && (
                  <label>
                    Diffusion worker URL
                    <input
                      placeholder="https://vllm.dataknife.net"
                      value={workerUrlOverride}
                      onChange={(e) => setWorkerUrlOverride(e.target.value)}
                    />
                    <span className="hint">
                      Overrides host/port. Leave blank and Save worker to clear the endpoint.
                    </span>
                  </label>
                )}
              </details>
            </div>
          </div>
        </section>
      )}

      {tab === "settings" && (
        <section className="panel settings">
          <p className="hint">
            {mock
              ? "Mock mode: keys are fake flags in memory. Paste anything non-empty to “configure” Civitai/OpenRouter. Agent/image models live in the compose chrome (Mode · Agent · Image)."
              : "Keys stay on the Freya API (or in your Kubernetes Secrets). They are never shown back in full after save."}{" "}
            OpenRouter alone is enough for cloud Generate. Local LoRAs / GPU worker live on the{" "}
            <button type="button" className="linkish" onClick={() => setTab("worker")}>
              Worker
            </button>{" "}
            tab (token, host/port, Start/Stop, base model).
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
            <span className="hint">Required for cloud image generation (no worker needed).</span>
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
              Search Civitai and (with a worker) install LoRAs/checkpoints. Pin works without a
              worker.
            </span>
          </label>

          <p className="hint">
            Models (OpenRouter via /v1/models): Agent picker has{" "}
            {Math.max(0, agentModels.length - 1)} chat model
            {agentModels.length === 2 ? "" : "s"}
            {agentModels.length > 1
              ? ` (e.g. ${agentModels
                  .filter((m) => m.id !== "openrouter/auto")
                  .slice(0, 2)
                  .map((m) => m.id)
                  .join(", ")})`
              : " — using offline fallbacks"}
            ; Image picker has {Math.max(0, imageModels.length - 1)} image model
            {imageModels.length === 2 ? "" : "s"}
            {imageModels.length > 1
              ? ` (e.g. ${imageModels
                  .filter((m) => m.id !== "auto")
                  .slice(0, 2)
                  .map((m) => m.id)
                  .join(", ")})`
              : ""}
            . Video:{" "}
            {videoModels.length > 0
              ? `${videoModels.length} discovered (e.g. ${videoModels
                  .slice(0, 2)
                  .map((m) => m.id)
                  .join(", ")}) — async gen not wired in UI yet.`
              : "none listed yet (needs OpenRouter key)."}
          </p>
          <button className="primary" disabled={busy} onClick={saveSettings}>
            Save settings
          </button>
          {savedMsg && <p className="hint">{savedMsg}</p>}
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
              <div className="row lora-card-actions">
                <button
                  type="button"
                  className={isFavorite(detailModel) ? "primary" : "ghost"}
                  onClick={() => toggleFavorite(detailModel)}
                >
                  {isFavorite(detailModel) ? "★ Favorited" : "☆ Favorite"}
                </button>
                <button
                  type="button"
                  className={isPinned(detailModel) ? "primary" : "ghost"}
                  onClick={() =>
                    isPinned(detailModel) ? unpinLora(detailModel) : pinLora(detailModel)
                  }
                >
                  {isPinned(detailModel) ? "Unpin" : "Pin to session"}
                </button>
                <button
                  type="button"
                  className="ghost"
                  disabled={busy}
                  onClick={() => installCivitai(detailModel)}
                >
                  {hasWorker() ? "Install" : "Install (needs worker)"}
                </button>
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
                    checkpoints for this base family so you can pin or install them too. Install
                    puts weights under FREYA_MODELS_DIR (game drive / PVC); set active base after.
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

/** Settings panel: list worker checkpoints + set active base for LoRA-compatible gens. */
function WorkerBaseModelPanel({ busy, hasWorker, api, onSaved, onError }) {
  const [checkpoints, setCheckpoints] = useState([]);
  const [catalog, setCatalog] = useState([]);
  const [active, setActive] = useState(null);
  const [selected, setSelected] = useState("");
  const [loading, setLoading] = useState(false);

  const refresh = async () => {
    if (!hasWorker) {
      setCheckpoints([]);
      setActive(null);
      return;
    }
    setLoading(true);
    try {
      const [cps, base, models] = await Promise.all([
        api("/v1/worker/checkpoints").catch(() => ({ data: [] })),
        api("/v1/worker/base-model").catch(() => null),
        api("/v1/base-models").catch(() => ({ items: [] })),
      ]);
      const list = Array.isArray(cps?.data) ? cps.data : [];
      setCheckpoints(list);
      setCatalog(Array.isArray(models?.items) ? models.items : []);
      setActive(base);
      if (base?.model) setSelected(base.model);
      else if (list[0]?.path) setSelected(list[0].path);
    } catch (e) {
      onError?.(e.message);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hasWorker]);

  const setBase = async () => {
    if (!selected.trim()) return;
    setLoading(true);
    try {
      const match = checkpoints.find((c) => c.path === selected);
      const catalogHit = catalog.find((c) => c.suggestedModel === selected);
      const body = {
        model: selected.trim(),
        baseModel: match?.baseModel || catalogHit?.baseModel || undefined,
      };
      const res = await api("/v1/worker/base-model", {
        method: "PUT",
        body: JSON.stringify(body),
      });
      setActive(res);
      onSaved?.(`Active base set to ${res.model || selected}`);
      await refresh();
    } catch (e) {
      onError?.(e.message);
    } finally {
      setLoading(false);
    }
  };

  if (!hasWorker) {
    return (
      <p className="hint">
        Set a local diffusion worker URL to host SD/SDXL/Pony/Flux checkpoints (LoRA bases). Remote
        text vLLM usually cannot run these.
      </p>
    );
  }

  const options = [
    ...checkpoints.map((c) => ({
      value: c.path,
      label: `${c.name || c.id}${c.baseModel ? ` (${c.baseModel})` : ""}`,
    })),
    ...catalog
      .filter((c) => !checkpoints.some((x) => x.path === c.suggestedModel))
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
      <p className="hint">
        LoRAs need a matching base on the worker disk (<code>checkpoints/</code>). Select active
        base for local generate; stub selects only — real GPU load is vLLM-Omni / engine.
      </p>
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
