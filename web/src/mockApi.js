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
  return {
    versionName: "v1.0",
    creator: "freya-mock",
    civitaiUrl: `https://civitai.com/models/${partial.id}`,
    description:
      partial.description ||
      `${partial.name} — mock Civitai asset for Freya. Compatible with ${partial.baseModel || "common"} bases.`,
    trainedWords: partial.triggerWord ? [partial.triggerWord] : [],
    stats: { downloadCount, thumbsUpCount },
    ...partial,
    thumbnailUrl: thumb,
    imageUrls: images,
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

function wantsImageIntent(message) {
  const m = String(message || "").toLowerCase();
  return [
    "generate",
    "make an image",
    "make me an image",
    "create an image",
    "draw ",
    "draw me",
    "render ",
    "paint ",
    "produce an image",
    "run session",
    "run a session",
    "make a picture",
    "create a picture",
  ].some((k) => m.includes(k));
}

function mockGenerateArtifact(prompt, body = {}) {
  const loras = Array.isArray(body.loras) ? body.loras : [];
  const loraBrief =
    loras.length > 0
      ? ` LoRAs: ${loras.map((l) => `${l.name}@${l.scale ?? 0.8}`).join(", ")}.`
      : "";
  const enhanced = modelBriefFor(prompt, loraBrief);
  const id = `mock-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
  const wantLocal =
    body.backend === "local" ||
    (body.backend !== "openrouter" &&
      loras.length > 0 &&
      Boolean(state.settings.diffusionWorkerUrl));
  const backend = wantLocal ? "local" : "openrouter";
  const specificImage = body.model && body.model !== "auto" ? body.model : null;
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
  if (wantsImageIntent(message)) {
    if (m.includes("run session") || m.includes("variations") || m.includes("batch")) {
      actions.push({ type: "run_session", args: { goal: message } });
    } else {
      actions.push({ type: "generate_image", args: { prompt: message, batch: false } });
    }
  }
  let reply;
  if (!actions.length) {
    reply =
      "Happy to talk through style, mood, and framing. Say the word when you want me to generate, search Civitai, or suggest ideas.";
  } else if (actions.some((a) => a.type === "generate_image" || a.type === "run_session")) {
    reply = "On it — generating that image for you.";
  } else if (actions.some((a) => a.type === "search_civitai")) {
    reply = "I’ll search Civitai for matching assets.";
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
      result: { query: q, loras },
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
  return { type: ty, status: "error", args, error: `unknown action: ${ty}` };
}

function isSideEffectAction(ty) {
  return ty === "generate_image" || ty === "install_asset" || ty === "run_session";
}

export async function mockApi(path, opts = {}) {
  await delay(
    path.includes("/run") || path.includes("/generate") || path.includes("/chat") ? 900 : 280,
  );
  const method = (opts.method || "GET").toUpperCase();
  const url = new URL(path, "http://mock.local");
  const p = url.pathname;

  if (p === "/v1/styles" && method === "GET") {
    return { items: state.styles };
  }

  if (p === "/v1/models" && method === "GET") {
    return {
      openrouter: [
        {
          id: "black-forest-labs/flux.2-klein-4b",
          name: "FLUX.2 Klein 4B",
          backend: "openrouter",
          modality: "image",
        },
      ],
      video: [
        {
          id: "google/veo-3.1",
          name: "Google: Veo 3.1",
          backend: "openrouter",
          modality: "video",
          supportedDurations: [4, 6, 8],
          supportedResolutions: ["720p", "1080p"],
          supportedAspectRatios: ["16:9", "9:16"],
        },
      ],
      local: [],
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
    return { ...state.settings };
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

  if (p === "/v1/chat" && method === "POST") {
    const body = parseBody(opts);
    const message = (body.message || "").trim() || BEGINNER_EXAMPLE;
    const mode = String(body.automationMode || "auto").toLowerCase();
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
    const loras = [];
    const artifacts = [];
    let civitaiQuery = "";
    for (const ar of actionResults) {
      const res = ar.result || {};
      if (Array.isArray(res.ideas)) ideas.push(...res.ideas);
      if (Array.isArray(res.loras)) loras.push(...res.loras);
      if (res.query) civitaiQuery = res.query;
      if (ar.type === "generate_image" && res.artifactId) artifacts.push(res);
      if (Array.isArray(res.results)) artifacts.push(...res.results);
    }

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
    return mockGenerateArtifact(body.prompt || "untitled", body);
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
    return { ok: true, destPath: body.destPath, name: body.name };
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
