/** In-memory Freya API mock for UI design without backend. */

function abortError() {
  const err = new Error("Aborted");
  err.name = "AbortError";
  return err;
}

/** Resolves after ms, or rejects with AbortError if signal aborts (search-as-you-type). */
const delay = (ms = 400, signal) =>
  new Promise((resolve, reject) => {
    if (signal?.aborted) {
      reject(abortError());
      return;
    }
    const t = setTimeout(resolve, ms);
    if (!signal) return;
    const onAbort = () => {
      clearTimeout(t);
      reject(abortError());
    };
    signal.addEventListener("abort", onAbort, { once: true });
  });

/**
 * Repo image sources for mock examples (viewed on disk):
 * - freya.png — README brand art: winged warrior, eagle feathers, leather armor, canyon waterfall
 * - freya-ui.png — legacy SwarmUI chrome (not used as a prompt seed)
 * - web/public/empty-gallery.png — opaque wink/parchment empty-gallery placeholder
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

const FREYA_ASSET_TYPES = new Set([
  "lora",
  "locon",
  "dora",
  "checkpoint",
  "textualinversion",
  "vae",
]);

function isFreyaAssetType(modelType) {
  return FREYA_ASSET_TYPES.has(String(modelType || "").toLowerCase());
}

function assetTypeRank(modelType) {
  const t = String(modelType || "").toLowerCase();
  if (t === "lora" || t === "locon" || t === "dora" || t.includes("lora")) return 0;
  if (t === "checkpoint") return 1;
  if (t === "textualinversion") return 2;
  if (t === "vae") return 3;
  return 4;
}

function mockCard(partial) {
  const thumb =
    partial.thumbnailUrl ||
    svgDataUrl(partial.name.slice(0, 28), partial.modelType || "asset", (partial.id % 300) + 10);
  const images = partial.imageUrls || [
    thumb,
    svgDataUrl(`${partial.name.slice(0, 18)} · 2`, partial.modelType || "", (partial.id % 300) + 40),
    svgDataUrl(`${partial.name.slice(0, 18)} · 3`, partial.modelType || "", (partial.id % 300) + 80),
  ];
  const downloadCount = 1200 + (partial.id % 50) * 137;
  const thumbsUpCount = 80 + (partial.id % 40) * 11;
  const versionId = partial.versionId || partial.id * 10 + 1;
  const downloadUrl =
    partial.downloadUrl || `https://civitai.com/api/download/models/mock-${partial.id}`;
  const baseModel = partial.baseModel || "SDXL 1.0";
  const trainedWords = partial.triggerWord ? [partial.triggerWord] : [];
  const primaryFile = {
    id: versionId * 100 + 1,
    name: `${(partial.name || "model").replace(/[^a-zA-Z0-9._-]+/g, "_")}.safetensors`,
    sizeKB: 150000 + (partial.id % 20) * 12000,
    sizeBytes: (150000 + (partial.id % 20) * 12000) * 1024,
    type: "Model",
    format: "SafeTensor",
    primary: true,
    downloadUrl,
  };
  const altFile = {
    id: versionId * 100 + 2,
    name: `${(partial.name || "model").replace(/[^a-zA-Z0-9._-]+/g, "_")}.ckpt`,
    sizeKB: 180000 + (partial.id % 20) * 12000,
    sizeBytes: (180000 + (partial.id % 20) * 12000) * 1024,
    type: "Model",
    format: "PickleTensor",
    primary: false,
    downloadUrl: `${downloadUrl}?format=ckpt`,
  };
  const v1 = {
    id: versionId,
    name: partial.versionName || "v1.0",
    baseModel,
    createdAt: "2024-06-01T00:00:00.000Z",
    trainedWords,
    files: [primaryFile, altFile],
    imageUrls: images,
    examplePrompt: partial.examplePrompt,
  };
  const v2 = {
    id: versionId + 1,
    name: "v1.1",
    baseModel,
    createdAt: "2024-09-15T00:00:00.000Z",
    trainedWords,
    files: [
      {
        ...primaryFile,
        id: (versionId + 1) * 100 + 1,
        name: primaryFile.name.replace(".safetensors", "_v11.safetensors"),
        downloadUrl: `${downloadUrl}?v=1.1`,
      },
    ],
    imageUrls: images.slice(0, 2),
    examplePrompt: partial.examplePrompt,
  };
  const modelVersions = partial.modelVersions || [v1, v2];
  const defaultFile = modelVersions[0]?.files?.find((f) => f.primary) || modelVersions[0]?.files?.[0];
  return {
    versionName: modelVersions[0]?.name || "v1.0",
    versionId: modelVersions[0]?.id || versionId,
    fileId: defaultFile?.id,
    fileName: defaultFile?.name,
    fileFormat: defaultFile?.format,
    creator: "freya-mock",
    civitaiUrl: `https://civitai.com/models/${partial.id}`,
    description:
      partial.description ||
      `${partial.name} — mock Civitai asset for Freya. Compatible with ${baseModel} bases.`,
    trainedWords,
    stats: { downloadCount, thumbsUpCount },
    modelVersions,
    ...partial,
    downloadUrl: partial.downloadUrl || defaultFile?.downloadUrl || downloadUrl,
    thumbnailUrl: thumb,
    imageUrls: images,
    baseModel,
  };
}

function toPublicCard(card) {
  const { tags: _t, ...rest } = card;
  return rest;
}

/** Mock Civitai catalog — brand demo query hits winged / fantasy warrior assets. */
const CIVITAI_CATALOG = [
  mockCard({
    id: 901,
    name: "Eagle Wing Warrior",
    modelType: "LORA",
    baseModel: "SDXL 1.0",
    downloadUrl: "https://civitai.com/api/download/models/mock-901",
    examplePrompt: FREYA_BRAND_EXAMPLE,
    thumbnailUrl: svgDataUrl("Eagle Wing Warrior", "LORA · fantasy", 28),
    triggerWord: "eaglewingwar",
    tags: ["fantasy", "winged", "warrior", "eagle", "wings", "armor", "character", "lora"],
  }),
  mockCard({
    id: 902,
    name: "Valkyrie Feather Armor",
    modelType: "LORA",
    baseModel: "SDXL 1.0",
    downloadUrl: "https://civitai.com/api/download/models/mock-902",
    examplePrompt:
      "A valkyrie in feathered wings and leather armor standing before a waterfall canyon",
    thumbnailUrl: svgDataUrl("Valkyrie Feather", "LORA · armor", 42),
    triggerWord: "valkfeather",
    tags: ["fantasy", "valkyrie", "winged", "warrior", "feathers", "armor", "canyon", "lora"],
  }),
  mockCard({
    id: 903,
    name: "Canyon Waterfall Scenic",
    modelType: "LORA",
    baseModel: "SDXL 1.0",
    downloadUrl: "https://civitai.com/api/download/models/mock-903",
    examplePrompt: "A lush green canyon with a multi-tier waterfall and wildflowers",
    thumbnailUrl: svgDataUrl("Canyon Waterfall", "LORA · landscape", 145),
    triggerWord: "canyonfall",
    tags: ["landscape", "canyon", "waterfall", "fantasy", "nature", "warrior", "lora"],
  }),
  mockCard({
    id: 101,
    name: "Yamer's Anime Illustrator",
    modelType: "Checkpoint",
    baseModel: "SDXL 1.0",
    downloadUrl: "https://civitai.com/api/download/models/mock-101",
    examplePrompt: ANIME_PORTRAIT_EXAMPLE,
    thumbnailUrl: svgDataUrl("Anime Illustrator", "Checkpoint", 310),
    triggerWord: null,
    tags: ["anime", "illustration", "portrait", "soft", "checkpoint"],
  }),
  mockCard({
    id: 202,
    name: "Studio Soft Light",
    modelType: "LORA",
    baseModel: "SD 1.5",
    downloadUrl: "https://civitai.com/api/download/models/mock-202",
    examplePrompt: "A portrait with soft studio light and shallow depth of field",
    thumbnailUrl: svgDataUrl("Studio Soft Light", "LORA · portrait", 200),
    triggerWord: "softstudiolight",
    tags: ["portrait", "studio", "light", "soft", "lighting", "photo", "character"],
  }),
  mockCard({
    id: 303,
    name: "Red Moon Landscape",
    modelType: "LORA",
    baseModel: "SDXL 1.0",
    downloadUrl: "https://civitai.com/api/download/models/mock-303",
    examplePrompt: ETHEREAL_MOON_EXAMPLE,
    thumbnailUrl: svgDataUrl("Red Moon", "LORA · night", 5),
    triggerWord: "redmoonscape",
    tags: ["landscape", "moon", "fantasy", "night", "mist", "ethereal"],
  }),
  mockCard({
    id: 404,
    name: "Harbor Fog Atmosphere",
    modelType: "LORA",
    baseModel: "SDXL 1.0",
    downloadUrl: "https://civitai.com/api/download/models/mock-404",
    examplePrompt: HARBOR_MOON_EXAMPLE,
    thumbnailUrl: svgDataUrl("Harbor Fog", "LORA · fog", 210),
    triggerWord: "harborfog",
    tags: ["harbor", "fog", "atmospheric", "water", "night", "moon"],
  }),
  mockCard({
    id: 505,
    name: "Ink Wash Drama",
    modelType: "LoCon",
    baseModel: "SDXL 1.0",
    downloadUrl: "https://civitai.com/api/download/models/mock-505",
    examplePrompt: "An ink wash painting with bold brush strokes",
    thumbnailUrl: svgDataUrl("Ink Wash Drama", "LoCon", 250),
    triggerWord: "inkwash",
    tags: ["ink", "painting", "drama", "illustration", "east"],
  }),
  mockCard({
    id: 606,
    name: "Character Consistency Pack",
    modelType: "LORA",
    baseModel: "Pony",
    downloadUrl: "https://civitai.com/api/download/models/mock-606",
    examplePrompt: LOCAL_CHARACTER_EXAMPLE,
    thumbnailUrl: svgDataUrl("Character Pack", "LORA", 180),
    triggerWord: "charpack",
    tags: ["character", "portrait", "consistency", "local", "studio"],
  }),
  mockCard({
    id: 707,
    name: "Winged Hero Embedding",
    modelType: "TextualInversion",
    baseModel: "SDXL 1.0",
    downloadUrl: "https://civitai.com/api/download/models/mock-707",
    examplePrompt: FREYA_BRAND_EXAMPLE,
    thumbnailUrl: svgDataUrl("Winged Hero", "Embedding", 35),
    triggerWord: "wingedhero-ti",
    tags: ["fantasy", "winged", "warrior", "embedding", "character"],
  }),
  mockCard({
    id: 808,
    name: "Soft Detail VAE",
    modelType: "VAE",
    baseModel: "SD 1.5",
    downloadUrl: "https://civitai.com/api/download/models/mock-808",
    examplePrompt: "Clean color and soft detail reconstruction",
    thumbnailUrl: svgDataUrl("Soft Detail VAE", "VAE", 160),
    triggerWord: null,
    tags: ["vae", "detail", "soft", "portrait", "anime"],
  }),
  mockCard({
    id: 909,
    name: "Fantasy Base XL",
    modelType: "Checkpoint",
    baseModel: "SDXL 1.0",
    downloadUrl: "https://civitai.com/api/download/models/mock-909",
    examplePrompt: FREYA_BRAND_EXAMPLE,
    thumbnailUrl: svgDataUrl("Fantasy Base XL", "Checkpoint", 55),
    triggerWord: null,
    tags: ["fantasy", "winged", "warrior", "checkpoint", "xl", "base"],
  }),
  mockCard({
    id: 910,
    name: "Pony Diffusion Mock",
    modelType: "Checkpoint",
    baseModel: "Pony",
    downloadUrl: "https://civitai.com/api/download/models/mock-910",
    examplePrompt: LOCAL_CHARACTER_EXAMPLE,
    thumbnailUrl: svgDataUrl("Pony Diffusion", "Checkpoint", 90),
    triggerWord: null,
    tags: ["pony", "checkpoint", "character", "base"],
  }),
];

function catalogPublic(c) {
  return toPublicCard(c);
}

function relatedFor(card) {
  if (!card || String(card.modelType).toLowerCase() === "checkpoint") return [];
  const base = card.baseModel;
  if (!base) return [];
  return CIVITAI_CATALOG.filter(
    (c) =>
      c.id !== card.id &&
      String(c.modelType).toLowerCase() === "checkpoint" &&
      c.baseModel === base,
  )
    .slice(0, 4)
    .map((c) => ({
      relation: "needs_checkpoint",
      note: `Compatible with ${base}`,
      model: catalogPublic(c),
    }));
}

const state = {
  installJobs: [],
  localAssets: [],
  settings: {
    openRouterConfigured: true,
    // Mock starts ready so Suggest/session can demo Civitai search + LoRA cards
    civitaiConfigured: true,
    hfConfigured: false,
    diffusionWorkerUrl: "",
    chatModel: "auto",
    imageModel: "auto",
    routerModel: "auto",
    apiTokenConfigured: false,
    adminLoginConfigured: true,
    authRequired: false,
    privacy: {
      zeroDataRetention: false,
      dataCollection: "allow",
      chatProviders: {
        excluded: [],
        legacyOnly: null,
      },
    },
    alignment: {
      modelAlignment: "balanced",
      allowNsfwLocal: false,
      instructionTone: "warm",
    },
    autoFallbacks: {
      agent: "",
      agentZdr: "",
      image: "",
      imageZdr: "",
      video: "",
      videoZdr: "",
      router: "",
      costTier: "medium",
    },
    instructions: [
      {
        id: "converse",
        label: "Converse (chat agent)",
        text: "You are Freya, a friendly agentic image studio guide (mock converse).",
        defaultText: "You are Freya, a friendly agentic image studio guide (mock converse).",
        overridden: false,
      },
      {
        id: "route",
        label: "Route (intent classifier)",
        text: "You are Freya's internal request router (mock).",
        defaultText: "You are Freya's internal request router (mock).",
        overridden: false,
      },
      {
        id: "chat",
        label: "Chat (assist)",
        text: "You are Freya, a friendly image studio agent (mock chat).",
        defaultText: "You are Freya, a friendly image studio agent (mock chat).",
        overridden: false,
      },
      {
        id: "plan",
        label: "Plan",
        text: "You are Freya's planner (mock).",
        defaultText: "You are Freya's planner (mock).",
        overridden: false,
      },
      {
        id: "suggest",
        label: "Suggest",
        text: "You are Freya suggest (mock).",
        defaultText: "You are Freya suggest (mock).",
        overridden: false,
      },
      {
        id: "session",
        label: "Session",
        text: "You are Freya session (mock).",
        defaultText: "You are Freya session (mock).",
        overridden: false,
      },
      {
        id: "prompt-prose",
        label: "Enhance — prose brief",
        text: "You are Freya enhance prose (mock).",
        defaultText: "You are Freya enhance prose (mock).",
        overridden: false,
      },
      {
        id: "prompt-sd",
        label: "Enhance — SD keywords",
        text: "You are Freya enhance SD (mock).",
        defaultText: "You are Freya enhance SD (mock).",
        overridden: false,
      },
    ],
  },
  /** mode → override text (empty/absent = pack default). */
  instructionOverrides: {},
  mockApiToken: "",
  mockAdminUser: "admin",
  mockAdminPass: "mock-admin",
  workerRuntime: { kind: "none", name: "mock", running: false, detail: "mock" },
  activeBaseModel: "",
  activeBaseFamily: "",
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
  const words = q.split(/\s+/).filter(Boolean).filter((w) => w !== "lora" && w !== "locon");
  const scored = CIVITAI_CATALOG.filter((c) => isFreyaAssetType(c.modelType)).map((c) => {
    const hay = `${c.name} ${c.modelType} ${(c.tags || []).join(" ")}`.toLowerCase();
    let score = 0;
    for (const w of words) {
      if (hay.includes(w)) score += 1;
    }
    // Prefer adapters when they already match the vibe
    if (score > 0) score += (3 - Math.min(assetTypeRank(c.modelType), 3)) * 0.25;
    return { c, score };
  })
    .filter((x) => x.score > 0)
    .sort((a, b) => {
      if (b.score !== a.score) return b.score - a.score;
      return assetTypeRank(a.c.modelType) - assetTypeRank(b.c.modelType);
    });

  const picked = (
    scored.length
      ? scored.map((x) => x.c)
      : CIVITAI_CATALOG.filter((c) => isFreyaAssetType(c.modelType)).sort(
          (a, b) => assetTypeRank(a.modelType) - assetTypeRank(b.modelType),
        )
  ).slice(0, limit);

  return picked.map(catalogPublic);
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

function mockLabelKey(raw) {
  const file = String(raw || "").split(/[/\\]/).pop() || "";
  const stem = file.replace(/\.(safetensors|ckpt|pt|gguf|bin)$/i, "");
  return stem.toLowerCase().replace(/[^a-z0-9]+/g, "");
}

function mockQueryHits(query, label) {
  const q = mockLabelKey(query);
  const n = mockLabelKey(label);
  if (q.length < 3 || !n) return false;
  if (q === n) return true;
  return (q.length >= 4 && n.includes(q)) || (n.length >= 4 && q.includes(n));
}

function mockSameAsset(local, search) {
  const lv = Number(local?.versionId);
  const sv = Number(search?.versionId);
  if (lv > 0 && sv > 0) return lv === sv;
  const localKeys = [mockLabelKey(local?.name), mockLabelKey(local?.path)].filter(Boolean);
  const searchKeys = [mockLabelKey(search?.name), mockLabelKey(search?.fileName)].filter(Boolean);
  return localKeys.some((k) => searchKeys.includes(k));
}

/** Installed name/version first, then Civitai hits that are not that file. */
function orderMockLookup(query, searchCards) {
  const installed = state.localAssets || [];
  const search = Array.isArray(searchCards) ? searchCards : [];
  const used = new Set();
  const consumed = new Set();
  const existing = [];
  const pushExisting = (local, hit) => {
    const path = local?.path || "";
    if (!path || used.has(path)) return;
    used.add(path);
    const modelId = local.modelId || (!hit?.path && /^\d+$/.test(String(hit?.id || "")) ? Number(hit.id) : null);
    existing.push({
      ...(hit || {}),
      ...local,
      modelId: modelId || local.modelId || null,
      versionId: local.versionId ?? hit?.versionId ?? null,
      thumbnailUrl: local.thumbnailUrl || hit?.thumbnailUrl || null,
      imageUrls: local.imageUrls?.length ? local.imageUrls : hit?.imageUrls,
      path,
      installed: true,
    });
  };
  if (mockLabelKey(query).length >= 3) {
    installed.forEach((local) => {
      if (mockQueryHits(query, local.name) || mockQueryHits(query, local.path)) {
        const idx = search.findIndex((s) => mockSameAsset(local, s));
        pushExisting(local, idx >= 0 ? search[idx] : null);
        if (idx >= 0) consumed.add(idx);
      }
    });
  }
  search.forEach((hit, i) => {
    if (consumed.has(i)) return;
    const local = installed.find((a) => mockSameAsset(a, hit));
    if (local) {
      pushExisting(local, hit);
      consumed.add(i);
    }
  });
  const fresh = search
    .filter((_, i) => !consumed.has(i))
    .map((hit) => ({ ...hit, installed: false }));
  return [...existing, ...fresh];
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

function containsAny(hay, needles) {
  return needles.some((n) => hay.includes(n));
}

function looksLikeTagPrompt(lower) {
  if (lower.includes("<lora:") || lower.includes("<lyco:") || lower.includes("(lora:")) return true;
  const commas = (lower.match(/,/g) || []).length;
  return commas >= 3 && !lower.includes("?");
}

function visualCueCount(lower) {
  const cues = [
    "cartoon",
    "meme",
    "illustration",
    "illustrated",
    "painting",
    "portrait",
    "sketch",
    "artwork",
    "comic",
    "anime",
    "watercolor",
    "oil painting",
    "background",
    "lighting",
    "wearing",
    "standing",
    "sitting",
    "close-up",
    "close up",
    "wide shot",
    "cinematic",
    "linework",
    "line art",
    "landscape",
    "sunset",
    "moonlight",
    "waterfall",
    "armor",
    "wings",
    "cowboy",
    "outline",
    "studio light",
    "digital art",
    "concept art",
    "photograph",
    "in the style",
    "masterpiece",
    "highly detailed",
    "1girl",
    "1boy",
    "photo of",
    "picture of",
    "image of",
    "drawing of",
    "shot of",
  ];
  return cues.filter((c) => lower.includes(c)).length;
}

/** Questions, how-it-works, and LoRA/model clarification — not a prompt to render. */
export function messageIsChatNotMedia(message) {
  const lower = String(message || "").trim().toLowerCase();
  if (!lower) return false;
  if (looksLikeTagPrompt(lower) && !lower.trimEnd().endsWith("?")) return false;
  const phrases = [
    "how does",
    "how do ",
    "how can ",
    "how would ",
    "what is ",
    "what are ",
    "what was ",
    "what's ",
    "whats ",
    "why does",
    "why is ",
    "why do ",
    "explain ",
    "tell me how",
    "tell me about",
    "tell me what",
    "which lora",
    "what lora",
    "which model",
    "what model",
    "which checkpoint",
    "what checkpoint",
    "which base",
    "what base",
    "clarify",
    "before we generate",
    "before generating",
    "don't generate",
    "do not generate",
    "not yet",
    "how it works",
    "how this works",
    "what should i",
    "should i use",
    "can you explain",
    "could you explain",
    "help me understand",
    "search civitai",
    "find a lora",
    "find loras",
    "suggest idea",
    "brainstorm",
    "list lora",
    "list checkpoint",
    "worker status",
  ];
  if (containsAny(lower, phrases)) return true;
  const words = lower.split(/\s+/).filter(Boolean);
  const first = words[0] || "";
  if (
    ["who", "what", "why", "how", "when", "where", "which", "whose"].includes(first) &&
    !(first === "how" && words[1] === "about") &&
    (lower.trimEnd().endsWith("?") || words.length < 14)
  ) {
    return true;
  }
  if (
    words.length < 24 &&
    (lower.startsWith("can you ") ||
      lower.startsWith("could you ") ||
      lower.startsWith("would you ") ||
      lower.startsWith("please explain"))
  ) {
    const imperative = containsAny(lower, [
      "draw",
      "generat",
      "illustrat",
      "render",
      "paint",
      "video",
      "gif",
      "animat",
      "picture",
      "image",
    ]);
    if (!imperative) return true;
  }
  return false;
}

function looksLikeProseScene(lower) {
  const stripped = lower.trim().replace(/^["'“”«»]+|["'“”«»]+$/g, "");
  if (stripped.includes("?")) return false;
  const words = stripped.split(/\s+/).filter(Boolean);
  if (words.length < 6) return false;
  const cues = visualCueCount(stripped);
  if (cues >= 2 && words.length >= 6) return true;
  if (cues >= 1 && words.length >= 8) return true;
  const startsScene =
    stripped.startsWith("a ") ||
    stripped.startsWith("an ") ||
    stripped.startsWith("prompt:") ||
    stripped.startsWith('"') ||
    stripped.startsWith("“");
  return startsScene && words.length >= 10 && (stripped.includes(",") || stripped.includes(" with "));
}

/** none | picture | series | gif | video — series is still a picture modality. */
export function directMediaAsk(message) {
  const lower = String(message || "").trim().toLowerCase();
  if (!lower || messageIsChatNotMedia(message)) return "none";
  const hasGif =
    /\bgif\b/.test(lower) || lower.includes(".gif") || lower.includes("animated gif");
  if (
    hasGif ||
    lower.includes("looping animation") ||
    lower.includes("animated meme") ||
    lower.includes("short animation") ||
    lower.includes("animate as gif")
  ) {
    return "gif";
  }
  if (
    containsAny(lower, [
      "video",
      "t2v",
      "i2v",
      "text-to-video",
      "image-to-video",
      "text to video",
      "image to video",
      "make a clip",
      "generate a clip",
      "motion clip",
      "animate this into",
      "short clip",
      "wan 2",
      "ltx video",
      "mp4",
      "webm",
    ]) ||
    (lower.includes("animat") && !lower.includes("still") && !hasGif)
  ) {
    return "video";
  }
  if (
    containsAny(lower, [
      "series of",
      "image series",
      "picture series",
      "set of images",
      "set of pictures",
      "set of stills",
      "storyboard",
      "sequence of images",
      "sequence of pictures",
      "several versions",
      "a few versions",
      "a few images",
      "a few pictures",
      "several images",
      "several pictures",
      "variations",
      "run session",
      "run a session",
      "batch of",
    ])
  ) {
    return "series";
  }
  if (
    containsAny(lower, [
      "generate",
      "make an image",
      "make me an image",
      "create an image",
      "make a picture",
      "create a picture",
      "draw ",
      "draw me",
      "render ",
      "paint ",
      "produce an image",
      "still image",
      "generate this",
      "image prompt",
    ]) ||
    lower.startsWith("prompt:")
  ) {
    return "picture";
  }
  if (looksLikeTagPrompt(lower) || looksLikeProseScene(lower)) return "picture";
  return "none";
}

/** Legacy-aligned modality: chat | picture | gif | video */
function inferModality(message) {
  const ask = directMediaAsk(message);
  if (ask === "gif") {
    return {
      modality: "gif",
      reason: "Heuristic: GIF / looping animation (legacy SwarmUI gif-hd).",
      source: "heuristic",
    };
  }
  if (ask === "video") {
    return {
      modality: "video",
      reason: "Heuristic: video / T2V / I2V (legacy SwarmUI video).",
      source: "heuristic",
    };
  }
  if (ask === "picture" || ask === "series") {
    return {
      modality: "picture",
      reason:
        ask === "series"
          ? "Heuristic: series of stills."
          : "Heuristic: still prompt (scene, meme, illustration, or quoted prompt).",
      source: "heuristic",
    };
  }
  return {
    modality: "chat",
    reason: "Heuristic: conversational.",
    source: "heuristic",
  };
}

function offerFromAsk(prompt, ask) {
  const text = String(prompt || "").trim();
  if (!text) return null;
  if (ask === "series") return { prompt: text, type: "run_session" };
  if (ask === "gif" || ask === "video") {
    return { prompt: text, type: "generate_video", modality: ask };
  }
  if (ask === "picture") return { prompt: text, type: "generate_image" };
  return null;
}

/** Quoted prompt in an assistant reply, when the reply itself is not the thing to draw. */
export function imagePromptFromReply(text) {
  const raw = String(text || "");
  const body = raw.split(/\n\s*Route:/i)[0].trim();
  if (!body) return null;
  const re = /["“]([^"”\n]{24,})["”]/g;
  let match;
  while ((match = re.exec(body))) {
    const q = match[1].trim();
    const ask = directMediaAsk(q);
    if (ask !== "none") return offerFromAsk(q, ask);
  }
  const labeled = body.match(/(?:image prompt|prompt)\s*:\s*(.+)$/im);
  if (labeled) {
    const line = labeled[1].trim().replace(/^["“]|["”]$/g, "");
    const ask = directMediaAsk(line);
    if (ask !== "none") return offerFromAsk(line, ask);
  }
  return null;
}

/** Generate button payload when this assistant turn did not already produce media. */
export function generateOfferForTurn(messages, index) {
  const list = Array.isArray(messages) ? messages : [];
  const m = list[index];
  if (!m || m.role === "user") return null;
  if (Array.isArray(m.artifactIds) && m.artifactIds.length) return null;
  const actions = Array.isArray(m.actions) ? m.actions : [];
  const done = actions.some(
    (a) =>
      (a.type === "generate_image" || a.type === "generate_video" || a.type === "run_session") &&
      a.status === "executed",
  );
  if (done) return null;
  const proposed = actions.find(
    (a) =>
      (a.type === "generate_image" || a.type === "generate_video" || a.type === "run_session") &&
      a.status === "proposed",
  );
  if (proposed) {
    const prompt = String(proposed.args?.prompt || proposed.args?.goal || "").trim();
    if (!prompt) return null;
    return {
      prompt,
      type: proposed.type,
      modality: proposed.args?.modality,
    };
  }
  const prevUser = [...list.slice(0, index)].reverse().find((x) => x.role === "user");
  if (prevUser) {
    const fromUser = offerFromAsk(prevUser.content, directMediaAsk(prevUser.content));
    if (fromUser) return fromUser;
  }
  return imagePromptFromReply(m.content);
}

function wantsImageIntent(message) {
  return inferModality(message).modality === "picture";
}

function mockGenerateArtifact(prompt, body = {}) {
  const loras = Array.isArray(body.loras) ? body.loras : [];
  const loraBrief =
    loras.length > 0
      ? ` LoRAs: ${loras.map((l) => `${l.name}@${l.scale ?? 0.8}`).join(", ")}.`
      : "";
  const enhanced = modelBriefFor(prompt, loraBrief);
  const id = `mock-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
  const modality =
    body.modality ||
    inferModality(prompt).modality ||
    "picture";
  const isMotion = modality === "gif" || modality === "video";
  const wantLocal =
    !isMotion &&
    (body.backend === "local" ||
      (body.backend !== "openrouter" &&
        loras.length > 0 &&
        Boolean(state.settings.diffusionWorkerUrl)));
  const backend = isMotion
    ? modality === "gif" && wantLocal
      ? "local"
      : "openrouter"
    : wantLocal
      ? "local"
      : "openrouter";
  const specificModel = body.model && body.model !== "auto" ? body.model : null;
  const mime =
    modality === "video" ? "video/mp4" : modality === "gif" ? "image/gif" : "image/png";
  const item = {
    id,
    prompt,
    enhancedPrompt: enhanced,
    backend,
    model: isMotion
      ? specificModel || "bytedance/seedance-2.5-flash"
      : backend === "local"
        ? "local-worker-stub"
        : specificModel || state.settings.imageModel || "black-forest-labs/flux.2-klein-4b",
    createdAt: new Date().toISOString(),
    mime,
    modality,
    url: svgDataUrl(
      prompt.slice(0, 40) || "Freya",
      isMotion ? modality : backend,
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
    modality,
    mime,
    url: item.url,
    meta: item,
    loras,
  };
}

function planChatActions(message, body) {
  const m = String(message || "").toLowerCase();
  const actions = [];
  if (m.includes("suggest") || m.includes("idea") || m.includes("brainstorm")) {
    actions.push({ type: "suggest_ideas", args: { topic: message } });
  }
  if (
    m.includes("civitai") ||
    m.includes("search for lora") ||
    m.includes("find a lora") ||
    m.includes("find loras") ||
    m.includes("search civitai")
  ) {
    const q =
      (message.split(/\bfor\b/i)[1] || message).trim().slice(0, 80) || BEGINNER_CIVITAI_QUERY;
    actions.push({ type: "search_civitai", args: { query: q } });
  }
  const asksSetBase =
    m.includes("set base") ||
    m.includes("switch base") ||
    m.includes("change base") ||
    m.includes("use checkpoint") ||
    m.includes("set checkpoint") ||
    m.includes("set active base") ||
    m.includes("switch checkpoint");
  const asksGetBase =
    m.includes("active base") ||
    m.includes("current base") ||
    m.includes("what base") ||
    m.includes("which base") ||
    m.includes("base model") ||
    m.includes("active checkpoint");
  const asksList =
    m.includes("list checkpoint") ||
    m.includes("list checkpoints") ||
    m.includes("available checkpoint") ||
    m.includes("which checkpoints") ||
    (m.includes("checkpoint") && (m.includes("list") || m.includes("show") || m.includes("available")));
  const asksStatus =
    (m.includes("worker") &&
      (m.includes("status") ||
        m.includes("reachable") ||
        m.includes("online") ||
        m.includes("on?") ||
        m.includes("off?"))) ||
    m.includes("is the worker");
  if (asksSetBase) {
    actions.push({ type: "list_checkpoints", args: {} });
    actions.push({ type: "get_base_model", args: {} });
  } else if (asksGetBase) actions.push({ type: "get_base_model", args: {} });
  else if (asksList) actions.push({ type: "list_checkpoints", args: {} });
  else if (asksStatus) actions.push({ type: "worker_status", args: {} });

  const mediaAsk = directMediaAsk(message);
  if (mediaAsk === "series" || (wantsImageIntent(message) && (m.includes("run session") || m.includes("variations") || m.includes("batch")))) {
    actions.push({ type: "run_session", args: { goal: message } });
  } else if (mediaAsk === "picture" || wantsImageIntent(message)) {
    actions.push({ type: "generate_image", args: { prompt: message, batch: false } });
  }
  let reply;
  if (!actions.length) {
    reply =
      "Happy to talk through style, mood, and framing. Say the word when you want me to generate, search Civitai, tweak the worker base, or suggest ideas.";
  } else if (actions.some((a) => a.type === "generate_image" || a.type === "run_session")) {
    reply = "On it — generating that image for you.";
  } else if (actions.some((a) => a.type === "search_civitai")) {
    reply = "I’ll search Civitai for matching assets.";
  } else if (
    actions.some((a) =>
      ["worker_status", "list_checkpoints", "get_base_model", "set_base_model"].includes(a.type),
    )
  ) {
    reply = "Checking worker settings…";
  } else {
    reply = "Here are a few directions we could take.";
  }
  if (body?.confirm && Array.isArray(body.actions) && body.actions.length) {
    return {
      reply: "Running the confirmed actions.",
      actions: body.actions.map((a) => ({
        type: a.type,
        args: a.args || {},
      })),
    };
  }
  return { reply, actions };
}

async function executeMockChatAction(action, body) {
  const ty = action.type;
  const args = action.args || {};
  if (ty === "suggest_ideas") {
    const topic = args.topic || body.message || BEGINNER_EXAMPLE;
    const ideas = suggestIdeas(topic);
    return {
      type: ty,
      status: "executed",
      args,
      result: {
        ideas,
        sessionNote: "Pick an idea and send it, or ask me to generate one.",
      },
    };
  }
  if (ty === "search_civitai") {
    const q = (args.query || "").trim() || BEGINNER_CIVITAI_QUERY;
    const { loras } = lorasForAssist(q);
    return {
      type: ty,
      status: "executed",
      args: { ...args, query: q },
      result: { query: q, loras: orderMockLookup(q, loras) },
    };
  }
  if (ty === "list_loras" || ty === "list_installed") {
    const locals = (state.localAssets || []).map((a) => ({ ...a, installed: true, backend: "local" }));
    if (ty === "list_loras") {
      return {
        type: ty,
        status: "executed",
        args,
        result: { loras: locals.filter((a) => a.kind === "lora" || !a.kind) },
      };
    }
    return {
      type: ty,
      status: "executed",
      args,
      result: {
        loras: locals.filter((a) => a.kind === "lora"),
        checkpoints: locals.filter((a) => a.kind === "checkpoint"),
        embeddings: locals.filter((a) => a.kind === "embedding"),
        vaes: locals.filter((a) => a.kind === "vae"),
      },
    };
  }
  if (ty === "generate_image") {
    const prompt = (args.prompt || body.message || BEGINNER_EXAMPLE).trim();
    const result = mockGenerateArtifact(prompt, {
      ...body,
      prompt,
      model: body.imageModel || body.model,
      loras: body.loras || [],
    });
    return { type: ty, status: "executed", args, result };
  }
  if (ty === "run_session") {
    const goal = (args.goal || body.message || BEGINNER_EXAMPLE).trim();
    const { batchPrompts } = sessionBatch(goal, body.session || {});
    const results = batchPrompts.map((p) =>
      mockGenerateArtifact(p, { ...body, prompt: p, loras: body.loras || [] }),
    );
    return {
      type: ty,
      status: "executed",
      args,
      result: { goal, batchPrompts, results },
    };
  }
  if (ty === "install_asset") {
    if (!state.settings.civitaiConfigured) {
      return { type: ty, status: "error", args, error: "Civitai API key required" };
    }
    if (!state.settings.diffusionWorkerUrl) {
      return {
        type: ty,
        status: "error",
        args,
        error: "LoRA install needs a diffusion worker",
      };
    }
    return {
      type: ty,
      status: "executed",
      args,
      result: { ok: true, destPath: args.destPath || "loras/asset.safetensors", name: args.name },
    };
  }
  if (ty === "worker_status") {
    const configured = Boolean(state.settings.diffusionWorkerUrl);
    return {
      type: ty,
      status: "executed",
      args,
      result: {
        workerConfigured: configured,
        reachable: configured,
        modelLoaded: configured ? state.activeBaseModel || null : null,
        engine: "mock",
      },
    };
  }
  if (ty === "list_checkpoints") {
    // Same as GET /v1/worker/checkpoints — only what was installed this session.
    return {
      type: ty,
      status: "executed",
      args,
      result: {
        checkpoints: (state.localAssets || []).filter((a) => a.kind === "checkpoint"),
      },
    };
  }
  if (ty === "get_base_model") {
    return {
      type: ty,
      status: "executed",
      args,
      result: {
        ok: true,
        model: state.activeBaseModel || "",
        baseModel: state.activeBaseFamily || null,
      },
    };
  }
  if (ty === "set_base_model") {
    const model = String(args.model || "").trim();
    if (!model) {
      return {
        type: ty,
        status: "error",
        args,
        error: "missing model path — list_checkpoints first",
      };
    }
    state.activeBaseModel = model;
    state.activeBaseFamily = args.baseModel || state.activeBaseFamily || "SDXL 1.0";
    return {
      type: ty,
      status: "executed",
      args,
      result: { ok: true, model, baseModel: state.activeBaseFamily, engine: "mock" },
    };
  }
  return { type: ty, status: "error", args, error: `unknown action: ${ty}` };
}

function isSideEffectAction(ty) {
  return (
    ty === "generate_image" ||
    ty === "install_asset" ||
    ty === "run_session" ||
    ty === "set_base_model"
  );
}

function enrichMockReplyWithWorker(reply, actionResults) {
  const lines = [];
  for (const ar of actionResults || []) {
    if (ar.status === "error") {
      lines.push(`${ar.type} failed: ${ar.error || "error"}`);
      continue;
    }
    if (ar.status === "proposed") {
      lines.push(`Proposed: ${ar.type}`);
      continue;
    }
    const res = ar.result || {};
    if (ar.type === "worker_status") {
      const parts = [];
      if (res.workerConfigured === false) parts.push("no worker URL");
      else if (res.workerConfigured) parts.push("URL configured");
      if (res.reachable === false) parts.push("not reachable");
      else if (res.reachable) parts.push("reachable");
      lines.push(parts.length ? `Worker: ${parts.join(", ")}.` : "Worker status checked.");
    } else if (ar.type === "list_checkpoints") {
      const items = res.checkpoints || [];
      if (!items.length) lines.push("No checkpoints on the worker yet.");
      else {
        lines.push(
          `Checkpoints (${items.length}): ${items
            .map((c) => `${c.name} [${c.baseModel || "?"}] (${c.path})`)
            .join("; ")}.`,
        );
      }
    } else if (ar.type === "get_base_model" || ar.type === "set_base_model") {
      const verb = ar.type === "set_base_model" ? "Active base set to" : "Active base is";
      lines.push(
        res.baseModel
          ? `${verb} ${res.model} (${res.baseModel}).`
          : `${verb} ${res.model || "(none)"}.`,
      );
    }
  }
  if (!lines.length) return reply;
  const block = lines.join(" ");
  const trimmed = String(reply || "").trim();
  if (!trimmed || trimmed === "Checking worker settings…") return block;
  return `${trimmed}\n\n${block}`;
}

export async function mockApi(path, opts = {}) {
  const signal = opts.signal;
  await delay(
    path.includes("/run") || path.includes("/generate") || path.includes("/chat") ? 900 : 280,
    signal,
  );
  if (signal?.aborted) throw abortError();
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
    const instructions = (state.settings.instructions || []).map((row) => {
      const ov = state.instructionOverrides?.[row.id];
      const overridden = Boolean(ov && String(ov).trim());
      return {
        ...row,
        text: overridden ? ov : row.defaultText,
        overridden,
      };
    });
    return { ...state.settings, instructions };
  }

  if (p === "/v1/metrics/openrouter" && method === "GET") {
    if (!state.settings.openRouterConfigured) {
      return {
        configured: false,
        keyError: "OpenRouter API key not configured",
        notes: [
          "Mock: paste any OpenRouter key in Settings to enable sample metrics.",
        ],
      };
    }
    return {
      configured: true,
      key: {
        label: "sk-or-…mock",
        usage: 12.34,
        usageDaily: 0.42,
        usageWeekly: 3.1,
        usageMonthly: 12.34,
        limit: 50,
        limitRemaining: 37.66,
        limitReset: "monthly",
        isFreeTier: false,
        isManagementKey: false,
      },
      credits: null,
      creditsError:
        "Only management keys can perform this operation (mock — normal key).",
      notes: [
        "GET /api/v1/key works with Freya’s normal completion key.",
        "GET /api/v1/credits requires an OpenRouter Management API key.",
        "OpenRouter does not expose a rich recent-transactions feed for completion keys.",
      ],
    };
  }

  if (p === "/v1/settings" && method === "PUT") {
    const body = parseBody(opts);
    if (body.civitaiApiKey !== undefined) {
      state.settings.civitaiConfigured = Boolean(String(body.civitaiApiKey || "").trim());
    }
    if (body.openrouterApiKey !== undefined) {
      state.settings.openRouterConfigured = Boolean(
        String(body.openrouterApiKey || "").trim(),
      );
    }
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
    return {
      ok: true,
      configured,
      url: state.settings.diffusionWorkerUrl || null,
      reachable: configured,
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

  if (p === "/v1/gallery" && method === "GET") {
    return { items: state.gallery };
  }

  const galleryOne = p.match(/^\/v1\/gallery\/([^/]+)$/);
  if (galleryOne && method === "GET") {
    const item = state.gallery.find((g) => g.id === galleryOne[1]);
    if (!item) throw new Error("not found");
    return item;
  }
  if (galleryOne && method === "DELETE") {
    const id = galleryOne[1];
    const idx = state.gallery.findIndex((g) => g.id === id);
    if (idx < 0) throw new Error("not found");
    state.gallery.splice(idx, 1);
    return { ok: true, id };
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

  if (p === "/v1/chat" && method === "POST") {
    const body = parseBody(opts);
    const message = (body.message || "").trim() || BEGINNER_EXAMPLE;
    const mode = String(body.automationMode || "auto").toLowerCase();
    const route = inferModality(message);
    if ((route.modality === "gif" || route.modality === "video") && !body.confirm) {
      const art = mockGenerateArtifact(message, {
        modality: route.modality,
        model: body.imageModel || body.model,
      });
      return {
        reply: `Generating ${route.modality} (mock).`,
        actions: [
          {
            type: "generate_video",
            status: "executed",
            args: { prompt: message, modality: route.modality },
            result: art,
          },
        ],
        pendingConfirmation: false,
        question: null,
        ideas: [],
        loras: [],
        civitaiQuery: null,
        artifacts: [art],
        automationMode: mode,
        route,
        resolvedModels: { video: art.model },
      };
    }
    const { reply: plannedReply, actions: planned } = planChatActions(message, body);
    let reply = plannedReply;
    const actionResults = [];
    let pendingConfirmation = false;
    let question = null;

    const sideEffects = planned.filter((a) => isSideEffectAction(a.type));
    if (mode === "plan" && !body.confirm && sideEffects.length) {
      pendingConfirmation = true;
      for (const a of planned) {
        if (isSideEffectAction(a.type)) {
          actionResults.push({ type: a.type, status: "proposed", args: a.args || {} });
        } else {
          actionResults.push(await executeMockChatAction(a, body));
        }
      }
      reply =
        reply ||
        "Here’s what I’d do — confirm to run generate/install, or keep chatting.";
    } else if (mode === "auto" && !body.confirm) {
      const installs = planned.filter((a) => a.type === "install_asset");
      if (installs.length > 1) {
        pendingConfirmation = true;
        question = {
          kind: "lora",
          text: "Several LoRA installs are queued — which should Freya install?",
        };
        for (const a of planned) {
          if (isSideEffectAction(a.type)) {
            actionResults.push({ type: a.type, status: "proposed", args: a.args || {} });
          } else {
            actionResults.push(await executeMockChatAction(a, body));
          }
        }
      } else {
        for (const a of planned) {
          actionResults.push(await executeMockChatAction(a, body));
        }
      }
    } else {
      for (const a of planned) {
        actionResults.push(await executeMockChatAction(a, body));
      }
    }

    const ideas = [];
    const artifacts = [];
    let civitaiQuery = "";
    let sawSearch = false;
    const searchLoras = [];
    const listed = [];
    for (const ar of actionResults) {
      const res = ar.result || {};
      if (Array.isArray(res.ideas)) ideas.push(...res.ideas);
      if (res.query) civitaiQuery = res.query;
      if (ar.type === "search_civitai" && Array.isArray(res.loras)) {
        sawSearch = true;
        searchLoras.push(...res.loras);
      } else if (ar.type === "list_loras" && Array.isArray(res.loras)) {
        listed.push(...res.loras);
      } else if (ar.type === "list_installed") {
        for (const key of ["loras", "checkpoints", "embeddings", "vaes"]) {
          if (Array.isArray(res[key])) listed.push(...res[key]);
        }
      }
      if (ar.type === "generate_image" && res.artifactId) artifacts.push(res);
      if (Array.isArray(res.results)) artifacts.push(...res.results);
    }
    const loras = sawSearch ? searchLoras : listed;

    reply = enrichMockReplyWithWorker(reply, actionResults);

    return {
      reply,
      actions: actionResults,
      pendingConfirmation,
      question,
      ideas,
      loras,
      civitaiQuery: civitaiQuery || null,
      artifacts,
      automationMode: mode,
      route,
    };
  }

  if (p === "/v1/assist" && method === "POST") {
    const body = parseBody(opts);
    const base = (body.message || "").trim() || BEGINNER_EXAMPLE;
    const mode = body.mode || "prompt-prose";

    const agentModel = body.model || state.settings.chatModel || "auto";
    const agentEcho =
      agentModel && agentModel !== "auto" && agentModel !== "openrouter/auto"
        ? ` (agent: ${agentModel})`
        : "";

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
    const route = inferModality(body.prompt || "");
    const modality = body.modality || route.modality;
    const isMotion = modality === "gif" || modality === "video";
    const model =
      body.model && body.model !== "auto"
        ? body.model
        : isMotion
          ? "bytedance/seedance-2.5-flash"
          : state.settings.imageModel === "auto"
            ? "black-forest-labs/flux.2-klein-4b"
            : state.settings.imageModel;
    return mockGenerateArtifact(body.prompt || "untitled", {
      ...body,
      model,
      modality,
    });
  }

  if (p === "/v1/civitai/search" && method === "GET") {
    if (!state.settings.civitaiConfigured) {
      throw new Error("Civitai API key not set — add it in Freya Settings");
    }
    const q = (url.searchParams.get("q") || "").toLowerCase();
    const typesRaw = (url.searchParams.get("types") || "").trim();
    const typeFilter = typesRaw
      ? new Set(
          typesRaw
            .split(",")
            .map((t) => t.trim().toLowerCase())
            .filter(Boolean),
        )
      : null;
    const inType = (c) =>
      typeFilter ? typeFilter.has(String(c.modelType || "").toLowerCase()) : isFreyaAssetType(c.modelType);
    const matched = CIVITAI_CATALOG.filter(
      (c) =>
        inType(c) &&
        (!q ||
          c.name.toLowerCase().includes(q) ||
          c.modelType.toLowerCase().includes(q) ||
          (c.tags || []).some((t) => t.includes(q)) ||
          q.split(/\s+/).some((w) => w && (c.tags || []).some((t) => t.includes(w)))),
    ).sort((a, b) => assetTypeRank(a.modelType) - assetTypeRank(b.modelType));
    const fallback = CIVITAI_CATALOG.filter(inType).sort(
      (a, b) => assetTypeRank(a.modelType) - assetTypeRank(b.modelType),
    );
    const items = (matched.length ? matched : fallback).map(catalogPublic);
    return { items };
  }

  {
    const m = p.match(/^\/v1\/civitai\/models\/(\d+)$/);
    if (m && method === "GET") {
      if (!state.settings.civitaiConfigured) {
        throw new Error("Civitai API key not set — add it in Freya Settings");
      }
      const id = Number(m[1]);
      const card = CIVITAI_CATALOG.find((c) => c.id === id);
      if (!card) throw new Error(`No model with id ${id}`);
      const needsBase =
        String(card.modelType).toLowerCase() === "checkpoint" || !card.baseModel
          ? null
          : `Needs base model family: ${card.baseModel}`;
      return {
        model: { ...catalogPublic(card), needsBase },
        related: relatedFor(card),
      };
    }
  }

  {
    const m = p.match(/^\/v1\/civitai\/model-versions\/(\d+)$/);
    if (m && method === "GET") {
      if (!state.settings.civitaiConfigured) {
        throw new Error("Civitai API key not set — add it in Freya Settings");
      }
      const versionId = Number(m[1]);
      const card =
        CIVITAI_CATALOG.find((c) => Number(c.versionId) === versionId) ||
        CIVITAI_CATALOG.find((c) =>
          (c.modelVersions || []).some((v) => Number(v.id) === versionId),
        );
      if (!card) throw new Error(`No model version with id ${versionId}`);
      const needsBase =
        String(card.modelType).toLowerCase() === "checkpoint" || !card.baseModel
          ? null
          : `Needs base model family: ${card.baseModel}`;
      return {
        model: {
          ...catalogPublic(card),
          versionId,
          needsBase,
        },
        related: relatedFor(card),
      };
    }
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
    const configured = Boolean(state.settings.diffusionWorkerUrl);
    return {
      ok: true,
      openRouter: state.settings.openRouterConfigured,
      civitai: state.settings.civitaiConfigured,
      diffusionWorker: configured,
      workerConfigured: configured,
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
