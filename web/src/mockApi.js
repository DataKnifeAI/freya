/** In-memory Freya API mock for UI design without backend. */

const delay = (ms = 400) => new Promise((r) => setTimeout(r, ms));

/**
 * Repo image sources for mock examples (viewed on disk):
 * - freya.png — README brand art: winged warrior, eagle feathers, leather armor, canyon waterfall
 * - freya-ui.png — legacy SwarmUI chrome (not used as a prompt seed)
 * - web HeroCanvas — foggy moonlit harbor silhouette (atelier chrome / gallery mood)
 *
 * Style examplePrompts below are beginner NL aimed at recreating those looks — not pack tag soup.
 */
const FREYA_BRAND_EXAMPLE =
  "A young woman with large brown eagle wings and fantasy leather armor, standing in a green canyon with a waterfall behind her";
const HARBOR_MOON_EXAMPLE =
  "A quiet harbor at blue hour with warm windows glowing through soft fog under a pale moon";
const ETHEREAL_MOON_EXAMPLE =
  "A mystical landscape under a radiant red moon — a twisted blossoming tree on rocky ground, misty mountains beyond a lake that mirrors the moonlight";
const LOCAL_CHARACTER_EXAMPLE =
  "A character portrait in soft studio light with a clear, detailed face — someone you’d recognize across scenes";
const ANIME_PORTRAIT_EXAMPLE =
  "An anime-style portrait with soft lighting, expressive eyes, and clean colorful linework";

/** Featured style[0] example — from freya.png brand art. */
export const BEGINNER_EXAMPLE = FREYA_BRAND_EXAMPLE;
/** Civitai search that matches freya.png vibe (wings + fantasy armor). */
export const BEGINNER_CIVITAI_QUERY = "fantasy winged warrior lora";

/** Mock /v1/styles — first entry is the default Use example / empty Suggest seed. */
const STYLE_PACKS = [
  {
    id: "freya-winged-warrior",
    displayName: "Freya (brand art)",
    tags: ["character", "fantasy", "wings"],
    examplePrompt: FREYA_BRAND_EXAMPLE,
    backend: "openrouter",
    openRouterModel: "black-forest-labs/flux.2-klein-4b",
    recommendedSize: "1024x1024",
  },
  {
    id: "ethereal-moon",
    displayName: "Ethereal Moon Landscape",
    tags: ["landscape", "fantasy", "moon"],
    examplePrompt: ETHEREAL_MOON_EXAMPLE,
    backend: "openrouter",
    openRouterModel: "black-forest-labs/flux.2-klein-4b",
    recommendedSize: "1024x1024",
  },
  {
    id: "harbor-moon",
    displayName: "Harbor Moon (atelier)",
    tags: ["harbor", "fog", "night"],
    examplePrompt: HARBOR_MOON_EXAMPLE,
    backend: "openrouter",
    openRouterModel: "black-forest-labs/flux.2-klein-4b",
    recommendedSize: "1024x768",
  },
  {
    id: "anime-portrait",
    displayName: "Anime Portrait (cloud)",
    tags: ["anime", "portrait"],
    examplePrompt: ANIME_PORTRAIT_EXAMPLE,
    backend: "openrouter",
    recommendedSize: "768x1024",
  },
  {
    id: "local-character-lora",
    displayName: "Local Character LoRA (worker)",
    tags: ["lora", "local", "character"],
    examplePrompt: LOCAL_CHARACTER_EXAMPLE,
    backend: "local",
    localCheckpoint: "stabilityai/stable-diffusion-xl-base-1.0",
    loras: [{ name: "my_character", scale: 0.8 }],
    triggerWords: "my_character",
    recommendedSize: "1024x1024",
  },
];

function svgDataUrl(title, subtitle, hue = 28) {
  const h2 = (hue + 48) % 360;
  const h3 = (hue + 110) % 360;
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="768" viewBox="0 0 1024 768">
  <defs>
    <linearGradient id="g" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="hsl(${hue},38%,22%)"/>
      <stop offset="48%" stop-color="hsl(${h2},30%,16%)"/>
      <stop offset="100%" stop-color="hsl(${h3},24%,10%)"/>
    </linearGradient>
    <radialGradient id="m" cx="78%" cy="22%" r="28%">
      <stop offset="0%" stop-color="hsla(42,70%,78%,0.55)"/>
      <stop offset="100%" stop-color="hsla(42,70%,78%,0)"/>
    </radialGradient>
  </defs>
  <rect width="1024" height="768" fill="url(#g)"/>
  <ellipse cx="800" cy="170" rx="180" ry="160" fill="url(#m)"/>
  <circle cx="800" cy="165" r="36" fill="hsla(42,75%,85%,0.85)"/>
  <path d="M0 420 C200 360 380 400 560 350 C740 300 900 370 1024 330 L1024 520 L0 520 Z" fill="hsla(200,20%,12%,0.55)"/>
  <rect y="500" width="1024" height="268" fill="hsla(200,18%,10%,0.65)"/>
  <rect x="420" y="300" width="70" height="210" fill="hsla(200,15%,8%,0.9)"/>
  <rect x="500" y="260" width="95" height="250" fill="hsla(200,15%,6%,0.95)"/>
  <rect x="440" y="340" width="14" height="18" fill="hsla(28,70%,55%,0.9)"/>
  <rect x="520" y="300" width="16" height="20" fill="hsla(32,75%,60%,0.95)"/>
  <rect x="550" y="340" width="16" height="20" fill="hsla(24,65%,48%,0.85)"/>
  <rect x="520" y="380" width="16" height="20" fill="hsla(30,70%,58%,0.8)"/>
  <text x="56" y="620" fill="#f3ebe1" font-family="Georgia, serif" font-size="36">${escapeXml(title.slice(0, 38))}</text>
  <text x="56" y="668" fill="#cfc4b4" font-family="system-ui,sans-serif" font-size="18" opacity="0.8">${escapeXml(subtitle.slice(0, 56))}</text>
</svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

function escapeXml(s) {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/** Expert SD-style: comma tags, weights, embeddings — Freya must not rewrite these away. */
function looksLikeSdPrompt(text) {
  const t = (text || "").trim();
  if (!t) return false;
  if (/\([^)]+:\d+(\.\d+)?\)/.test(t)) return true;
  if (/\[[^\]]+:\d+(\.\d+)?\]/.test(t)) return true;
  if (/\b(lora|lyco)\s*:/i.test(t)) return true;
  if (/<(?:lora|lyco):[^>]+>/i.test(t)) return true;
  const commas = (t.match(/,/g) || []).length;
  if (commas >= 3 && !/[.!?]\s/.test(t)) return true;
  return false;
}

/** Mock Civitai catalog — brand demo query hits winged / fantasy warrior LoRAs. */
const CIVITAI_CATALOG = [
  {
    id: 901,
    name: "Eagle Wing Warrior",
    modelType: "LORA",
    downloadUrl: "https://civitai.com/api/download/models/mock-901",
    examplePrompt: FREYA_BRAND_EXAMPLE,
    thumbnailUrl: null,
    triggerWord: "eaglewingwar",
    tags: ["fantasy", "winged", "warrior", "eagle", "wings", "armor", "character", "lora"],
  },
  {
    id: 902,
    name: "Valkyrie Feather Armor",
    modelType: "LORA",
    downloadUrl: "https://civitai.com/api/download/models/mock-902",
    examplePrompt:
      "A valkyrie in feathered wings and leather armor standing before a waterfall canyon",
    thumbnailUrl: null,
    triggerWord: "valkfeather",
    tags: ["fantasy", "valkyrie", "winged", "warrior", "feathers", "armor", "canyon", "lora"],
  },
  {
    id: 903,
    name: "Canyon Waterfall Scenic",
    modelType: "LORA",
    downloadUrl: "https://civitai.com/api/download/models/mock-903",
    examplePrompt: "A lush green canyon with a multi-tier waterfall and wildflowers",
    thumbnailUrl: null,
    triggerWord: "canyonfall",
    tags: ["landscape", "canyon", "waterfall", "fantasy", "nature", "warrior", "lora"],
  },
  {
    id: 101,
    name: "Yamer's Anime Illustrator",
    modelType: "Checkpoint",
    downloadUrl: "https://civitai.com/api/download/models/mock-101",
    examplePrompt: ANIME_PORTRAIT_EXAMPLE,
    thumbnailUrl: null,
    triggerWord: null,
    tags: ["anime", "illustration", "portrait", "soft"],
  },
  {
    id: 202,
    name: "Studio Soft Light",
    modelType: "LORA",
    downloadUrl: "https://civitai.com/api/download/models/mock-202",
    examplePrompt: "A portrait with soft studio light and shallow depth of field",
    thumbnailUrl: null,
    triggerWord: "softstudiolight",
    tags: ["portrait", "studio", "light", "soft", "lighting", "photo", "character"],
  },
  {
    id: 303,
    name: "Red Moon Landscape",
    modelType: "LORA",
    downloadUrl: "https://civitai.com/api/download/models/mock-303",
    examplePrompt: ETHEREAL_MOON_EXAMPLE,
    thumbnailUrl: null,
    triggerWord: "redmoonscape",
    tags: ["landscape", "moon", "fantasy", "night", "mist", "ethereal"],
  },
  {
    id: 404,
    name: "Harbor Fog Atmosphere",
    modelType: "LORA",
    downloadUrl: "https://civitai.com/api/download/models/mock-404",
    examplePrompt: HARBOR_MOON_EXAMPLE,
    thumbnailUrl: null,
    triggerWord: "harborfog",
    tags: ["harbor", "fog", "atmospheric", "water", "night", "moon"],
  },
  {
    id: 505,
    name: "Ink Wash Drama",
    modelType: "LoCon",
    downloadUrl: "https://civitai.com/api/download/models/mock-505",
    examplePrompt: "An ink wash painting with bold brush strokes",
    thumbnailUrl: null,
    triggerWord: "inkwash",
    tags: ["ink", "painting", "drama", "illustration", "east"],
  },
  {
    id: 606,
    name: "Character Consistency Pack",
    modelType: "LORA",
    downloadUrl: "https://civitai.com/api/download/models/mock-606",
    examplePrompt: LOCAL_CHARACTER_EXAMPLE,
    thumbnailUrl: null,
    triggerWord: "charpack",
    tags: ["character", "portrait", "consistency", "local", "studio"],
  },
];

const state = {
  settings: {
    openRouterConfigured: true,
    // Mock starts ready so Suggest/session can demo Civitai search + LoRA cards
    civitaiConfigured: true,
    hfConfigured: false,
    diffusionWorkerUrl: "",
    chatModel: "openrouter/auto",
    imageModel: "black-forest-labs/flux.2-klein-4b",
  },
  styles: STYLE_PACKS,
  gallery: [
    {
      id: "mock-seed-1",
      prompt: HARBOR_MOON_EXAMPLE,
      enhancedPrompt:
        "Quiet harbor at blue hour, lantern-lit windows along the pier, soft fog over still water under a pale moon, cinematic atmosphere, cohesive cool palette with warm accents",
      backend: "openrouter",
      model: "black-forest-labs/flux.2-klein-4b",
      createdAt: new Date(Date.now() - 3600_000).toISOString(),
      url: svgDataUrl("Quiet harbor", "blue hour · fog", 210),
      loras: [],
    },
  ],
  civitai: CIVITAI_CATALOG,
};

function parseBody(opts) {
  if (!opts?.body) return {};
  try {
    return JSON.parse(opts.body);
  } catch {
    return {};
  }
}

function matchLoras(query, limit = 6) {
  const q = (query || "").toLowerCase();
  const words = q.split(/\s+/).filter(Boolean);
  const scored = CIVITAI_CATALOG.map((c) => {
    const hay = `${c.name} ${c.modelType} ${(c.tags || []).join(" ")}`.toLowerCase();
    let score = 0;
    for (const w of words) {
      if (hay.includes(w)) score += 1;
    }
    const isLora =
      c.modelType.toLowerCase().includes("lora") || c.modelType.toLowerCase() === "locon";
    // Only boost LoRAs that already matched query terms (avoid unrelated LoRA noise)
    if (score > 0 && isLora) score += 0.5;
    return { c, score };
  })
    .filter((x) => x.score > 0)
    .sort((a, b) => b.score - a.score);

  const picked = (
    scored.length
      ? scored.map((x) => x.c)
      : CIVITAI_CATALOG.filter((c) => c.modelType.toLowerCase().includes("lora"))
  ).slice(0, limit);

  return picked.map(({ tags: _t, triggerWord, ...rest }) => ({
    ...rest,
    ...(triggerWord ? { triggerWord } : {}),
  }));
}

/** Map example / goal text → Civitai query. Brand freya.png path is explicit. */
function civitaiQueryFromText(text) {
  const t = (text || "").toLowerCase();
  // freya.png / Freya (brand art) demo — wings + armor / canyon waterfall
  if (
    /wing|eagle|valkyr|armor|canyon/.test(t) ||
    (/waterfall/.test(t) && /woman|girl|warrior|character/.test(t))
  ) {
    return BEGINNER_CIVITAI_QUERY;
  }
  if (/anime|manga|illustration|linework/.test(t)) return "anime illustration lora";
  if (/moon|mist|mountain|landscape|ethereal|blossoming/.test(t)) return "fantasy landscape moon";
  if (/harbor|fog|pier|blue hour/.test(t)) return "atmospheric fog harbor";
  if (/portrait|face|character|studio/.test(t)) return "portrait soft light lora";
  if (/ink|wash|brush|painting/.test(t)) return "ink wash painting";
  return "atmospheric style lora";
}

/** Exported so the UI can preview the same Civitai query Suggest would use. */
export function demoCivitaiQuery(text = BEGINNER_EXAMPLE) {
  return civitaiQueryFromText(text);
}

function suggestIdeas(base) {
  const seed = base.trim() || "an atmospheric scene";
  // Everyday language only — never tag soup
  return [
    `${seed} — early morning light, quiet and unhurried.`,
    `${seed} — dusk instead, warmer lamps and longer shadows.`,
    `${seed} — after rain, reflections on the ground, cooler air.`,
    `A tighter crop on the same idea: focus on one detail that carries the mood.`,
  ].slice(0, 4);
}

function sessionBatch(base, session) {
  const goal =
    (session?.goal && String(session.goal).trim()) ||
    base.trim() ||
    "Explore an atmospheric scene until the look feels right";
  const pinned = Array.isArray(session?.pinnedLoras) ? session.pinnedLoras : [];
  const pinNote =
    pinned.length > 0
      ? ` Using pinned LoRA${pinned.length > 1 ? "s" : ""}: ${pinned.map((l) => l.name).join(", ")}.`
      : "";
  return {
    goal,
    batchPrompts: [
      `${goal} — wide establishing shot, soft natural light.`,
      `${goal} — closer framing, richer detail on the main subject.`,
      `${goal} — alternate mood: cooler palette, quieter atmosphere.`,
      pinned.length
        ? `${goal} — lean into the pinned style (${pinned[0].name}), keep the description conversational.`
        : `${goal} — golden-hour variation with warmer highlights.`,
    ].slice(0, pinned.length ? 4 : 3),
    pinNote,
  };
}

function lorasForAssist(civitaiQuery) {
  // Mock always returns matches so Suggest/session can demo the Civitai bridge.
  // Install needs civitai + worker; Pin and OpenRouter generate work without a worker.
  const loras = matchLoras(civitaiQuery, 6);
  if (!state.settings.civitaiConfigured) {
    return {
      loras,
      note: "Mock preview of Civitai matches — add any non-empty key in Settings to search/install.",
    };
  }
  if (!state.settings.diffusionWorkerUrl) {
    return {
      loras,
      note: "LoRA suggestions ready — pin anytime. Install needs a worker; Generate still uses OpenRouter.",
    };
  }
  return { loras, note: null };
}

function modelBriefFor(prompt, loraBrief) {
  const trimmed = prompt.trim();
  if (looksLikeSdPrompt(trimmed)) {
    // Pass through expert prompting — do not paraphrase into prose
    return trimmed + loraBrief;
  }
  return `A carefully composed scene: ${trimmed}. Natural light and atmosphere, clear focal subject, cohesive color and mood throughout.${loraBrief}`;
}

export async function mockApi(path, opts = {}) {
  await delay(path.includes("/run") || path.includes("/generate") ? 900 : 280);
  const method = (opts.method || "GET").toUpperCase();
  const url = new URL(path, "http://mock.local");
  const p = url.pathname;

  if (p === "/v1/styles" && method === "GET") {
    return { items: state.styles };
  }

  if (p === "/v1/models" && method === "GET") {
    const align = String(state.settings?.alignment?.modelAlignment || "balanced").toLowerCase();
    const zdr = Boolean(state.settings?.privacy?.zeroDataRetention);
    const allowNsfw = Boolean(state.settings?.alignment?.allowNsfwLocal);
    const chatAll = [
      {
        id: "anthropic/claude-sonnet-5.5",
        name: "Anthropic: Claude Sonnet 5.5",
        backend: "openrouter",
        modality: "chat",
        moderated: true,
        major: true,
      },
      {
        id: "google/gemini-4-neon",
        name: "Google: Gemini 4 Neon",
        backend: "openrouter",
        modality: "chat",
        moderated: true,
        major: true,
      },
      {
        id: "openai/gpt-4.1-mini",
        name: "OpenAI: GPT-4.1 Mini",
        backend: "openrouter",
        modality: "chat",
        moderated: true,
        major: true,
      },
      {
        id: "deepseek/deepseek-v4.1-flash-1017",
        name: "DeepSeek: DeepSeek V4.1 Flash",
        backend: "openrouter",
        modality: "chat",
        moderated: false,
        major: false,
      },
      {
        id: "cognitivecomputations/dolphin-llama-3-70b",
        name: "Dolphin Llama 3 70B (uncensored)",
        backend: "openrouter",
        modality: "chat",
        moderated: false,
        major: false,
        uncensored: true,
      },
    ];
    const available = [
      ...new Set(
        chatAll
          .map((m) => String(m.id || "").split("/")[0]?.toLowerCase())
          .filter(Boolean),
      ),
    ].sort();
    const cp = state.settings?.privacy?.chatProviders || {};
    const excluded = Array.isArray(cp.excluded)
      ? cp.excluded.map((s) => String(s).toLowerCase())
      : [];
    const legacyOnly = Array.isArray(cp.legacyOnly)
      ? cp.legacyOnly.map((s) => String(s).toLowerCase())
      : null;
    const vendorOk = (id) => {
      const v = String(id || "").split("/")[0]?.toLowerCase() || "";
      if (legacyOnly && legacyOnly.length) return legacyOnly.includes(v);
      return !excluded.includes(v);
    };
    let chat = chatAll.filter((m) => {
      if (!vendorOk(m.id)) return false;
      if (align === "safety") return Boolean(m.major || m.moderated) && !m.uncensored;
      if (align === "uncensored") return Boolean(m.uncensored) || !m.moderated;
      return true;
    });
    // ZDR catalog shrink — mock stands in for OpenRouter `?zdr=true`.
    if (zdr) {
      chat = chat.filter((m) => m.major || m.id.startsWith("anthropic/"));
    }
    let openrouter = [
      {
        id: "black-forest-labs/flux.2-klein-4b",
        name: "FLUX.2 Klein 4B",
        backend: "openrouter",
        modality: "image",
        zdr: false,
      },
      {
        id: "google/gemini-2.5-flash-image",
        name: "Google: Gemini 2.5 Flash Image",
        backend: "openrouter",
        modality: "image",
        zdr: true,
      },
      {
        id: "openai/gpt-image-1-mini",
        name: "OpenAI: GPT Image 1 Mini",
        backend: "openrouter",
        modality: "image",
        zdr: false,
      },
      {
        id: "bytedance-seed/seedream-5-5",
        name: "ByteDance Seed: Seedream 5.5",
        backend: "openrouter",
        modality: "image",
        zdr: false,
      },
    ];
    let video = [
      {
        id: "google/veo-3.1",
        name: "Google: Veo 3.1",
        backend: "openrouter",
        modality: "video",
        zdr: true,
        supportedDurations: [4, 6, 8],
        supportedResolutions: ["720p", "1080p"],
        supportedAspectRatios: ["16:9", "9:16"],
      },
      {
        id: "bytedance/seedance-2.5-flash",
        name: "ByteDance: Seedance 2.5 Flash",
        backend: "openrouter",
        modality: "video",
        zdr: false,
        supportedDurations: [4, 5],
        supportedResolutions: ["720p"],
        supportedAspectRatios: ["16:9", "9:16"],
      },
    ];
    if (zdr) {
      openrouter = openrouter.filter((m) => m.zdr);
      video = video.filter((m) => m.zdr);
    }
    // Same rows the worker list endpoints return — not a frozen demo catalog.
    const installedLocal = (state.localAssets || []).map((a) => ({
      id: a.id,
      name: a.name,
      backend: "local",
      kind: a.kind || "checkpoint",
      path: a.path,
      baseModel: a.baseModel || null,
      modelType: a.modelType || null,
    }));
    const local = allowNsfw
      ? installedLocal
      : installedLocal.filter(
          (m) => !/nsfw|xxx|porn|explicit/i.test(`${m.id} ${m.name} ${m.path}`),
        );
    return {
      openrouter: openrouter.map(({ zdr: _z, ...row }) => row),
      chat: chat.map(({ moderated: _m, major: _j, uncensored: _u, ...row }) => row),
      router: chat.map(({ moderated: _m, major: _j, uncensored: _u, ...row }) => row),
      video: video.map(({ zdr: _z, ...row }) => row),
      local,
      chatProviders: {
        available,
        excluded,
        legacyOnly,
      },
    };
  }

  if (p === "/v1/settings" && method === "GET") {
    return { ...state.settings };
  }

  if (p === "/v1/settings" && method === "PUT") {
    const body = parseBody(opts);
    if (body.civitaiApiKey) state.settings.civitaiConfigured = true;
    if (body.openrouterApiKey) state.settings.openRouterConfigured = true;
    if (body.diffusionWorkerUrl !== undefined) {
      state.settings.diffusionWorkerUrl = body.diffusionWorkerUrl;
    }
    if (body.chatModel) state.settings.chatModel = body.chatModel;
    if (body.imageModel) state.settings.imageModel = body.imageModel;
    if (body.routerModel) state.settings.routerModel = body.routerModel;
    if (body.privacy && typeof body.privacy === "object") {
      const next = {
        ...state.settings.privacy,
        ...body.privacy,
      };
      if (body.privacy.chatProviders && typeof body.privacy.chatProviders === "object") {
        const cp = body.privacy.chatProviders;
        const excluded = Array.isArray(cp.excluded)
          ? [...new Set(cp.excluded.map((s) => String(s || "").trim().toLowerCase()).filter(Boolean))].sort()
          : [];
        const legacyOnly =
          Array.isArray(cp.legacyOnly) && cp.legacyOnly.length
            ? [...new Set(cp.legacyOnly.map((s) => String(s || "").trim().toLowerCase()).filter(Boolean))].sort()
            : null;
        next.chatProviders = { excluded, legacyOnly };
        delete next.providerOnly;
        delete next.providerIgnore;
      }
      state.settings.privacy = next;
    }
    if (body.autoFallbacks && typeof body.autoFallbacks === "object") {
      state.settings.autoFallbacks = {
        ...state.settings.autoFallbacks,
        ...body.autoFallbacks,
      };
    }
    if (body.alignment && typeof body.alignment === "object") {
      state.settings.alignment = {
        ...state.settings.alignment,
        ...body.alignment,
      };
    }
    if (body.instructionOverrides && typeof body.instructionOverrides === "object") {
      if (!state.instructionOverrides) state.instructionOverrides = {};
      for (const [id, text] of Object.entries(body.instructionOverrides)) {
        if (text == null || !String(text).trim()) {
          delete state.instructionOverrides[id];
        } else {
          state.instructionOverrides[id] = String(text);
        }
      }
    }
    const instructions = (state.settings.instructions || []).map((row) => {
      const ov = state.instructionOverrides?.[row.id];
      const overridden = Boolean(ov && String(ov).trim());
      return {
        ...row,
        text: overridden ? ov : row.defaultText,
        overridden,
      };
    });
    state.settings.instructions = instructions;
    return { ...state.settings, instructions, persisted: true };
  }

  if (p === "/v1/auth/status" && method === "GET") {
    return {
      authRequired: Boolean(state.settings.authRequired),
      adminLoginConfigured: Boolean(state.settings.adminLoginConfigured),
      apiTokenConfigured: Boolean(state.settings.apiTokenConfigured),
      openRouterConfigured: Boolean(state.settings.openRouterConfigured),
      adminUsername: state.mockAdminUser || "admin",
      note: "Mock mode — sign-in optional.",
    };
  }

  if (p === "/v1/auth/login" && method === "POST") {
    const body = parseBody(opts);
    if (
      body.username !== state.mockAdminUser ||
      body.password !== state.mockAdminPass
    ) {
      const err = new Error("Invalid username or password");
      err.status = 401;
      throw err;
    }
    const bytes = new Uint8Array(32);
    if (typeof crypto !== "undefined" && crypto.getRandomValues) {
      crypto.getRandomValues(bytes);
    } else {
      for (let i = 0; i < bytes.length; i++) bytes[i] = (Math.random() * 256) | 0;
    }
    const token = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
    state.mockApiToken = token;
    state.settings.apiTokenConfigured = true;
    return {
      token,
      username: state.mockAdminUser,
      note: "Signed in.",
    };
  }

  if (p === "/v1/auth/token" && method === "POST") {
    const rotated = Boolean(state.mockApiToken);
    const bytes = new Uint8Array(32);
    if (typeof crypto !== "undefined" && crypto.getRandomValues) {
      crypto.getRandomValues(bytes);
    } else {
      for (let i = 0; i < bytes.length; i++) bytes[i] = (Math.random() * 256) | 0;
    }
    const token = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
    state.mockApiToken = token;
    state.settings.apiTokenConfigured = true;
    return {
      token,
      rotated,
      persisted: true,
      secretSynced: false,
      note: "Machine API token created.",
      syncCommands: null,
    };
  }

  if (p === "/v1/worker/stats" && method === "GET") {
    const configured = Boolean(state.settings.diffusionWorkerUrl);
    const enabled = state.settings.workerEnabled !== false;
    return {
      ok: true,
      enabled,
      configured,
      url: state.settings.diffusionWorkerUrl || null,
      reachable: configured && enabled,
      runtime: state.workerRuntime,
      vram: null,
      host: null,
      loras: configured ? 2 : null,
      checkpoints: configured ? 1 : null,
      uptimeSec: configured ? 42 : null,
      modelLoaded: configured
        ? {
            model: "checkpoints/mock-sdxl.safetensors",
            baseModel: "SDXL 1.0",
            loadedInVram: false,
            engine: "diffusers",
          }
        : null,
      modelsDir: configured ? "/models" : null,
      modelsDirHost: configured ? "/mnt/game2/freya/models" : null,
      engine: configured
        ? {
            ok: true,
            engine: "diffusers",
            cuda: true,
            loadedInVram: false,
            keepLoaded: false,
          }
        : null,
      vramBehavior: configured
        ? "on-demand load during generate; unload after unless FREYA_ENGINE_KEEP_LOADED=1"
        : null,
      layout: {
        loras: "loras/*.safetensors",
        checkpoints: "checkpoints/*.safetensors",
        activeBase: "active-base.json",
      },
      storage: {
        gallery: {
          path: "/data/gallery",
          usedBytes: 1.2 * 1024 ** 3,
          freeBytes: 18.8 * 1024 ** 3,
          totalBytes: 20 * 1024 ** 3,
          usedPercent: 6,
        },
        models: configured
          ? {
              path: "/mnt/game2/freya/models",
              usedBytes: 48 * 1024 ** 3,
              freeBytes: 452 * 1024 ** 3,
              totalBytes: 500 * 1024 ** 3,
              usedPercent: 10,
            }
          : null,
      },
      worker: {},
    };
  }

  if (p === "/v1/worker/checkpoints" && method === "GET") {
    // Only return what was actually installed in this mock session — no seed/demo cards.
    return {
      data: (state.localAssets || []).filter((a) => a.kind === "checkpoint"),
    };
  }

  if (p === "/v1/worker/loras" && method === "GET") {
    return { data: (state.localAssets || []).filter((a) => a.kind === "lora") };
  }

  if (p === "/v1/worker/embeddings" && method === "GET") {
    return {
      data: (state.localAssets || []).filter(
        (a) => a.kind === "embedding" || a.kind === "textualinversion",
      ),
    };
  }

  if (p === "/v1/worker/vae" && method === "GET") {
    return { data: (state.localAssets || []).filter((a) => a.kind === "vae") };
  }

  if (p === "/v1/worker/assets" && method === "DELETE") {
    const body = parseBody(opts);
    state.localAssets = (state.localAssets || []).filter((a) => a.path !== body.path);
    if (state.activeBaseModel === body.path) {
      state.activeBaseModel = "";
      state.activeBaseFamily = "";
    }
    return { ok: true, path: body.path };
  }

  if (p === "/v1/worker/base-model" && method === "GET") {
    return {
      ok: true,
      model: state.activeBaseModel || "",
      baseModel: state.activeBaseFamily || null,
      modelsDir: "/mnt/game2/freya/models",
    };
  }

  if (p === "/v1/worker/base-model" && method === "PUT") {
    const body = parseBody(opts);
    state.activeBaseModel = body.model || state.activeBaseModel || "checkpoints/mock-sdxl.safetensors";
    state.activeBaseFamily = body.baseModel || state.activeBaseFamily || "SDXL 1.0";
    return {
      ok: true,
      model: state.activeBaseModel,
      baseModel: state.activeBaseFamily,
      loadedInVram: false,
      engine: "stub",
    };
  }

  if (p === "/v1/base-models" && method === "GET") {
    return {
      items: [
        {
          baseModel: "SDXL 1.0",
          family: "SDXL",
          suggestedModel: "stabilityai/stable-diffusion-xl-base-1.0",
          note: "mock",
          localOnly: true,
        },
        {
          baseModel: "Pony",
          family: "Pony",
          suggestedModel: "checkpoints/pony-diffusion-v6-xl.safetensors",
          note: "mock",
          localOnly: true,
        },
      ],
      note: "mock catalog",
    };
  }

  if (p === "/v1/worker/start" && method === "POST") {
    state.workerRuntime = { kind: "mock", name: "mock", running: true, detail: "mock start" };
    return { ok: true, runtime: state.workerRuntime };
  }
  if (p === "/v1/worker/stop" && method === "POST") {
    state.workerRuntime = { kind: "mock", name: "mock", running: false, detail: "mock stop" };
    return { ok: true, runtime: state.workerRuntime };
  }
  if (p === "/v1/worker/enable" && method === "POST") {
    state.settings.workerEnabled = true;
    return { ok: true, enabled: true };
  }
  if (p === "/v1/worker/disable" && method === "POST") {
    state.settings.workerEnabled = false;
    return { ok: true, enabled: false };
  }

  if (p === "/v1/gallery" && method === "GET") {
    return { items: state.gallery };
  }

  const galleryOne = p.match(/^\/v1\/gallery\/([^/]+)$/);
  if (galleryOne && method === "GET") {
    const item = state.gallery.find((g) => g.id === galleryOne[1]);
    if (!item) throw new Error("not found");
    return item;
  }

  const galleryImage = p.match(/^\/v1\/gallery\/([^/]+)\/image$/);
  if (galleryImage && method === "GET") {
    const item = state.gallery.find((g) => g.id === galleryImage[1]);
    if (!item) throw new Error("not found");
    return { __redirect: item.url };
  }

  const remix = p.match(/^\/v1\/gallery\/([^/]+)\/remix$/);
  if (remix && method === "POST") {
    const item = state.gallery.find((g) => g.id === remix[1]);
    if (!item) throw new Error("not found");
    return {
      prompt: item.prompt,
      enhancedPrompt: item.enhancedPrompt,
      backend: item.backend,
      model: item.model,
    };
  }

  if (p === "/v1/assist" && method === "POST") {
    const body = parseBody(opts);
    const base = (body.message || "").trim() || BEGINNER_EXAMPLE;
    const mode = body.mode || "prompt-prose";

    const agentModel = body.model || state.settings.chatModel || "openrouter/auto";
    const agentEcho =
      agentModel && agentModel !== "openrouter/auto" ? ` (agent: ${agentModel})` : "";

    if (mode === "suggest") {
      const ideas = suggestIdeas(base);
      const civitaiQuery = civitaiQueryFromText(base);
      const { loras, note } = lorasForAssist(civitaiQuery);
      const sessionNote =
        (note ||
          "Pick an idea that feels closest, pin a LoRA if you like the vibe, then Generate or Run session for a set.") +
        agentEcho;
      const plan = { ideas, civitaiQuery, sessionNote };
      return {
        mode,
        text: JSON.stringify(plan, null, 2),
        plan,
        ideas,
        civitaiQuery,
        sessionNote,
        loras,
        model: agentModel,
        automationMode: body.automationMode || "plan",
      };
    }

    if (mode === "session" || mode === "plan") {
      const session = body.session || {};
      const { goal, batchPrompts, pinNote } = sessionBatch(base, session);
      const civitaiQuery = civitaiQueryFromText(`${base} ${goal}`);
      const { loras, note } = lorasForAssist(civitaiQuery);
      const sessionNote =
        (note ||
          `Here's a small set exploring your goal.${pinNote} Pick a favorite and iterate — Freya remembers this session.`) +
        agentEcho;
      const steps = [
        {
          kind: "search_loras",
          prompt: "",
          note: civitaiQuery ? `Search Civitai for “${civitaiQuery}”` : "Skip LoRA search",
        },
        {
          kind: "generate",
          prompt: batchPrompts[0] || base,
          note: "Generate a batch of natural-language variations",
        },
        {
          kind: "iterate",
          prompt: "",
          note: "Review the set and refine from session memory",
        },
      ];
      const plan = { goal, steps, civitaiQuery, batchPrompts, sessionNote };
      return {
        mode,
        text: JSON.stringify(plan, null, 2),
        plan,
        goal,
        steps,
        batchPrompts,
        civitaiQuery,
        sessionNote,
        loras,
        model: agentModel,
        automationMode: body.automationMode || "plan",
      };
    }

    // Legacy prose refine — still prose, not keyword tags (UI no longer exposes this)
    if (looksLikeSdPrompt(base)) {
      return { mode, text: base };
    }
    return {
      mode,
      text: `${base} Soft directional light, a rich sense of place, and a cohesive mood — keep the description conversational and specific.`,
    };
  }

  if ((p === "/v1/run" || p === "/v1/generate") && method === "POST") {
    const body = parseBody(opts);
    const prompt = body.prompt || "untitled";
    const loras = Array.isArray(body.loras) ? body.loras : [];
    const loraBrief =
      loras.length > 0
        ? ` LoRAs: ${loras.map((l) => `${l.name}@${l.scale ?? 0.8}`).join(", ")}.`
        : "";
    const enhanced = modelBriefFor(prompt, loraBrief);
    const id = `mock-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
    // Worker optional: OpenRouter when no worker (or backend auto/openrouter). Local only when
    // explicitly requested or LoRAs are applied on a configured worker.
    const wantLocal =
      body.backend === "local" ||
      (body.backend !== "openrouter" &&
        loras.length > 0 &&
        Boolean(state.settings.diffusionWorkerUrl));
    const backend = wantLocal ? "local" : "openrouter";
    const specificImage =
      body.model && body.model !== "auto" ? body.model : null;
    const item = {
      id,
      prompt,
      enhancedPrompt: enhanced,
      backend,
      model:
        backend === "local"
          ? "local-worker-stub"
          : specificImage || state.settings.imageModel || "black-forest-labs/flux.2-klein-4b",
      createdAt: new Date().toISOString(),
      url: svgDataUrl(
        prompt.slice(0, 40) || "Freya",
        backend,
        backend === "local" ? 140 : 28 + (id.length % 40),
      ),
      loras,
    };
    state.gallery.unshift(item);
    return {
      artifactId: id,
      backend: item.backend,
      model: item.model,
      enhancedPrompt: enhanced,
      url: item.url,
      meta: item,
      loras,
    };
  }

  if (p === "/v1/civitai/search" && method === "GET") {
    if (!state.settings.civitaiConfigured) {
      throw new Error("Civitai API key not set — add it in Freya Settings");
    }
    const q = (url.searchParams.get("q") || "").toLowerCase();
    const items = CIVITAI_CATALOG.filter(
      (c) =>
        !q ||
        c.name.toLowerCase().includes(q) ||
        c.modelType.toLowerCase().includes(q) ||
        (c.tags || []).some((t) => t.includes(q)),
    ).map(({ tags: _t, ...rest }) => rest);
    return { items: items.length ? items : CIVITAI_CATALOG.map(({ tags: _t, ...rest }) => rest) };
  }

  if (p === "/v1/civitai/install" && method === "POST") {
    if (!state.settings.civitaiConfigured) {
      throw new Error("Civitai API key required — set it in Settings");
    }
    if (!state.settings.diffusionWorkerUrl) {
      throw new Error(
        "LoRA install needs a diffusion worker — set the worker URL in Settings (OpenRouter Generate still works without one)",
      );
    }
    const body = parseBody(opts);
    const dest = body.destPath || "";
    const activeDup = (state.installJobs || []).find(
      (j) =>
        j.destPath === dest && ["queued", "downloading"].includes(j.status),
    );
    if (activeDup) {
      throw new Error(
        `already downloading to ${dest} — wait or cancel the existing job`,
      );
    }
    if ((state.localAssets || []).some((a) => a.path === dest)) {
      throw new Error(`already installed at ${dest} — delete it first`);
    }
    const jobId = `mock-${Date.now()}`;
    const job = {
      id: jobId,
      status: "queued",
      destPath: body.destPath,
      name: body.name,
      baseModel: body.baseModel,
      modelType: body.modelType,
      bytesDone: 0,
      bytesTotal: 1024 * 1024,
      percent: 0,
      createdAt: Math.floor(Date.now() / 1000),
      updatedAt: Math.floor(Date.now() / 1000),
    };
    state.installJobs = [job, ...(state.installJobs || [])].slice(0, 40);
    setTimeout(() => {
      const j = (state.installJobs || []).find((x) => x.id === jobId);
      if (!j || j.status === "cancelled") return;
      j.status = "downloading";
      j.bytesDone = 512 * 1024;
      j.percent = 50;
      j.updatedAt = Math.floor(Date.now() / 1000);
      setTimeout(() => {
        const done = (state.installJobs || []).find((x) => x.id === jobId);
        if (!done || done.status === "cancelled") return;
        done.status = "succeeded";
        done.bytesDone = done.bytesTotal;
        done.percent = 100;
        done.updatedAt = Math.floor(Date.now() / 1000);
        state.localAssets = state.localAssets || [];
        const mt = String(body.modelType || "LORA").toLowerCase();
        const path = String(body.destPath || "");
        const kind = path.startsWith("checkpoints/") || mt.includes("checkpoint")
          ? "checkpoint"
          : path.startsWith("embeddings/") || mt.includes("textual")
            ? "embedding"
            : path.startsWith("vae/") || mt === "vae"
              ? "vae"
              : "lora";
        state.localAssets.push({
          id: (body.name || "asset").replace(/\W+/g, "_"),
          name: body.name || "asset",
          kind,
          path: body.destPath,
          baseModel: body.baseModel,
          modelType: body.modelType,
          modelId: body.modelId ?? null,
          versionId: body.versionId ?? null,
          thumbnailUrl: body.thumbnailUrl ?? null,
          sizeBytes: done.bytesTotal,
          source: "civitai",
        });
        if (kind === "checkpoint" && !state.activeBaseModel) {
          state.activeBaseModel = path;
          state.activeBaseFamily = body.baseModel || "";
        }
      }, 800);
    }, 400);
    return {
      ok: true,
      queued: true,
      jobId,
      status: "queued",
      job,
      destPath: body.destPath,
      name: body.name,
    };
  }

  if (p === "/v1/civitai/jobs" && method === "GET") {
    return { data: state.installJobs || [] };
  }

  if (p === "/v1/civitai/jobs/clear" && method === "POST") {
    const includeQueued = Boolean(body?.includeQueued);
    const before = state.installJobs || [];
    const next = before.filter((j) => {
      const st = String(j?.status || "").toLowerCase();
      if (["succeeded", "failed", "cancelled"].includes(st)) return false;
      if (includeQueued && st === "queued") return false;
      return true;
    });
    const removed = before.length - next.length;
    state.installJobs = next;
    return { ok: true, removed, data: next };
  }

  if (p.startsWith("/v1/civitai/jobs/") && p.endsWith("/cancel") && method === "POST") {
    const id = decodeURIComponent(p.slice("/v1/civitai/jobs/".length, -"/cancel".length));
    const job = (state.installJobs || []).find((j) => j.id === id);
    if (!job) throw new Error("job not found");
    if (["queued", "downloading"].includes(job.status)) {
      job.status = "cancelled";
      job.error = "cancelled";
      job.updatedAt = Math.floor(Date.now() / 1000);
    }
    return { ok: true, job };
  }

  if (p.startsWith("/v1/civitai/jobs/") && method === "GET") {
    const id = decodeURIComponent(p.slice("/v1/civitai/jobs/".length));
    const job = (state.installJobs || []).find((j) => j.id === id);
    if (!job) throw new Error("job not found");
    return { job };
  }

  if (p === "/v1/health" || p === "/healthz") {
    return {
      ok: true,
      openRouter: state.settings.openRouterConfigured,
      civitai: state.settings.civitaiConfigured,
      diffusionWorker: Boolean(state.settings.diffusionWorkerUrl),
      mock: true,
    };
  }

  throw new Error(`mock: unhandled ${method} ${p}`);
}

export function isMockMode() {
  if (import.meta.env.VITE_FREYA_MOCK === "true") return true;
  if (typeof window !== "undefined") {
    const q = new URLSearchParams(window.location.search);
    if (q.get("mock") === "1") return true;
    if (localStorage.getItem("freya.mock") === "1") return true;
  }
  return false;
}
