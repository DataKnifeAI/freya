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
  const [workerUrl, setWorkerUrl] = useState("");
  const [civitaiQ, setCivitaiQ] = useState(BEGINNER_CIVITAI_QUERY);
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
  const sessionsRef = useRef(sessions);
  const activeIdRef = useRef(activeSessionId);
  const timelineRef = useRef(null);

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
    const onChange = (e) => setTheme(e.matches ? "light" : "dark");
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
      setWorkerUrl(st.diffusionWorkerUrl || "");
      setGallery(g.items || []);
      const featured = items[0]?.examplePrompt;
      if (featured) {
        setCivitaiQ((prev) =>
          !prev.trim() || prev === BEGINNER_CIVITAI_QUERY ? demoCivitaiQuery(featured) : prev,
        );
      }
    } catch (e) {
      setError(e.message);
    }
  };

  useEffect(() => {
    refresh();
  }, []);

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

  const hasWorker = () => Boolean(settings?.diffusionWorkerUrl || workerUrl.trim());
  const canAutoInstall = () => Boolean(settings?.civitaiConfigured && hasWorker());
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
      return {
        ...prev,
        pinnedLoras: [...(prev.pinnedLoras || []), next],
        sessionNote: item.triggerWord
          ? `Pinned “${item.name}”. Trigger word “${item.triggerWord}” is optional — Freya keeps your prompt as you wrote it.`
          : `Pinned “${item.name}” for this session. Your prompt stays as you wrote it.`,
      };
    });
    setSavedMsg(`Pinned ${item.name} for this session.`);
  };

  const unpinLora = (item) => {
    setSession((prev) => ({
      ...prev,
      pinnedLoras: (prev.pinnedLoras || []).filter((p) => loraKey(p) !== loraKey(item)),
    }));
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

  const runBody = (nlPrompt) => {
    const body = {
      prompt: nlPrompt,
      enhance: "auto",
      backend: "auto",
      loras: pinnedPayload(),
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
    const results = [];
    for (const bp of batch) {
      const data = await runOne(bp);
      results.push({ ...data, prompt: bp });
    }
    setBatchResults(results);
    setResult(results[0] || null);
    setModelBrief(results[0]?.enhancedPrompt || "");
    setPrompt(batch[0] || goal);
    pushTurn(goal, results.map((r) => r.artifactId).filter(Boolean));
    const g = await api("/v1/gallery");
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
    setBusy(true);
    setError("");
    setBatchResults([]);
    try {
      const data = await runOne(prompt);
      setResult(data);
      setModelBrief(data.enhancedPrompt || "");
      pushTurn(prompt, data.artifactId ? [data.artifactId] : []);
      const g = await api("/v1/gallery");
      setGallery(g.items || []);
    } catch (e) {
      setError(e.message);
    } finally {
      setBusy(false);
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
    setBusy(true);
    setError("");
    setBatchResults([]);
    setSessionLog([]);
    setPendingQuestion(null);
    try {
      const plan = await fetchSessionPlan();

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
      setError(e.message);
    } finally {
      setBusy(false);
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
      setSavedMsg(
        mock
          ? "Mock settings saved in-memory (reload resets unless you keep this tab open)."
          : "Settings saved. Keys are stored for this Freya API instance.",
      );
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

  const renderLoraCard = (item, { compact = false } = {}) => {
    const pinned = isPinned(item);
    return (
      <article key={loraKey(item)} className={`lora-card${compact ? " compact" : ""}`}>
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

  const showAssistPanel =
    ideas.length > 0 || suggestedLoras.length > 0 || suggestNote || civitaiQuery;

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
            <button type="button" className="ghost mock-banner-btn" onClick={leaveMock}>
              Exit
            </button>
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
              <span className={`status-dot ${settings.diffusionWorkerUrl ? "on" : ""}`} />
              Worker
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
                <button type="button" className="t-prompt" onClick={() => setPrompt(t.prompt)}>
                  {t.prompt}
                </button>
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
                {suggestedLoras.length > 0 && (
                  <p className="hint">
                    {suggestedLoras.length} LoRA candidate{suggestedLoras.length === 1 ? "" : "s"}{" "}
                    in the gallery pane → pin or install there.
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
          {suggestedLoras.length > 0 && (
            <aside className="lora-strip" aria-label="LoRA selection preview">
              <div className="lora-strip-head">
                <div>
                  <span className="t-kind">LoRA selection</span>
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
                Pin anytime (no worker needed). Install needs a worker — otherwise Freya generates
                via OpenRouter.
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
          <label>
            Diffusion worker URL (optional — LoRAs / local custom look)
            <input
              placeholder="http://gpu-box.lan:8091"
              value={workerUrl}
              onChange={(e) => setWorkerUrl(e.target.value)}
            />
            <span className="hint">
              Optional. Needed to install LoRAs and run local/custom models. Leave blank to generate
              only via OpenRouter.
            </span>
          </label>
          <button className="primary" disabled={busy} onClick={saveSettings}>
            Save settings
          </button>
          {savedMsg && <p className="hint">{savedMsg}</p>}
        </section>
      )}
        </section>
      </div>
      </div>
    </div>
  );
}
