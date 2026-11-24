#!/usr/bin/env python3
"""Freya local diffusion engine (Diffusers + CUDA).

Speaks a tiny HTTP API on FREYA_ENGINE_PORT (default 8092). freya-worker proxies
/v1/images/generations here. Loads checkpoints/LoRAs from FREYA_MODELS_DIR on
demand; unloads after each generate unless FREYA_ENGINE_KEEP_LOADED=1.
"""

from __future__ import annotations

import base64
import gc
import io
import logging
import os
import re
import secrets
import threading
from pathlib import Path
from typing import Any

import torch
import uvicorn
from fastapi import FastAPI, HTTPException
from fastapi.responses import JSONResponse
from pydantic import BaseModel, Field

LOG = logging.getLogger("freya-engine")

MODELS_DIR = Path(os.environ.get("FREYA_MODELS_DIR", "/models")).resolve()
_A1111_INLINE_TAG = re.compile(r"<(?:lora|lyco):[^>]+>", re.IGNORECASE)
# Civitai / A1111 emphasis and attention weights — plain Diffusers ignores these.
_A1111_ATTENTION = re.compile(r"\([^)]*:\s*\d+(?:\.\d+)?\)|\([^):]+\)")
KEEP_LOADED = os.environ.get("FREYA_ENGINE_KEEP_LOADED", "0").strip() in (
    "1",
    "true",
    "yes",
)
DEFAULT_STEPS = int(os.environ.get("FREYA_ENGINE_STEPS", "20"))
DEFAULT_GUIDANCE = float(os.environ.get("FREYA_ENGINE_GUIDANCE", "7.0"))
HF_HOME = os.environ.get("HF_HOME") or os.environ.get("HUGGINGFACE_HUB_CACHE")

app = FastAPI(title="freya-diffusers-engine", version="0.1.0")
_lock = threading.Lock()
_pipe: Any = None
_loaded_model: str | None = None
_loaded_loras: list[str] = []


class GenerateReq(BaseModel):
    prompt: str
    model: str | None = None
    negative_prompt: str | None = None
    size: str | None = None
    seed: int | None = None
    steps: int | None = None
    guidance_scale: float | None = None
    cfg: float | None = None
    sampler: str | None = None
    clip_skip: int | None = Field(default=None, alias="clipSkip")
    quantity: int | None = None
    n: int | None = None
    lora_request: list[dict[str, Any]] | None = Field(default=None)
    base_model: str | None = Field(default=None, alias="baseModel")
    frames: int | None = None
    format: str | None = None

    model_config = {"populate_by_name": True}


def _device() -> str:
    return "cuda" if torch.cuda.is_available() else "cpu"


def _dtype():
    if torch.cuda.is_available():
        return torch.float16
    return torch.float32


def _parse_size(size: str | None) -> tuple[int, int]:
    if not size:
        return 1024, 1024
    s = size.lower().replace("*", "x")
    if "x" in s:
        a, b = s.split("x", 1)
        return max(64, int(a)), max(64, int(b))
    n = max(64, int(s))
    return n, n


def _looks_xl(model: str, base_model: str | None) -> bool:
    blob = f"{model} {base_model or ''}".lower()
    keys = (
        "xl",
        "sdxl",
        "pony",
        "illustrious",
        "noobai",
        "juggernaut",
        "realvis",
        "hassaku",
        "wai-",
    )
    return any(k in blob for k in keys)


def _looks_sd15(model: str, base_model: str | None) -> bool:
    blob = f"{model} {base_model or ''}".lower()
    if _looks_xl(model, base_model):
        return False
    return any(k in blob for k in ("sd 1.5", "sd1.5", "1.5", "dreamshaper_8", "dreamshaper_-_8"))


def _resolve_model_path(model: str) -> str:
    """Return HF id or absolute path under MODELS_DIR."""
    m = model.strip()
    if not m:
        raise HTTPException(status_code=400, detail="model is required")
    if m.endswith(".safetensors") or m.startswith("checkpoints/") or "/" in m and not m.startswith("http"):
        # Relative under models dir, or absolute path already in tree.
        p = Path(m)
        if not p.is_absolute():
            p = MODELS_DIR / m
        p = p.resolve()
        if not str(p).startswith(str(MODELS_DIR)):
            raise HTTPException(status_code=400, detail="model path escapes models dir")
        if not p.is_file():
            raise HTTPException(status_code=404, detail=f"checkpoint not found: {m}")
        return str(p)
    # Hugging Face repo id (org/name)
    return m


def _resolve_lora_path(name: str) -> Path:
    """Accept lora id, filename, or loras/... path."""
    raw = name.strip()
    candidates: list[Path] = []
    if raw.startswith("loras/") or raw.endswith(".safetensors"):
        candidates.append(MODELS_DIR / raw if not Path(raw).is_absolute() else Path(raw))
    else:
        candidates.append(MODELS_DIR / "loras" / f"{raw}.safetensors")
        candidates.append(MODELS_DIR / "loras" / raw)
        if not raw.endswith(".safetensors"):
            candidates.append(MODELS_DIR / "loras" / f"{raw}.safetensors")
    for c in candidates:
        try:
            p = c.resolve()
        except OSError:
            continue
        if str(p).startswith(str(MODELS_DIR)) and p.is_file():
            return p
    raise HTTPException(status_code=404, detail=f"lora not found: {name}")


def _unload_locked() -> None:
    global _pipe, _loaded_model, _loaded_loras
    if _pipe is not None:
        try:
            del _pipe
        except Exception:
            pass
    _pipe = None
    _loaded_model = None
    _loaded_loras = []
    gc.collect()
    if torch.cuda.is_available():
        torch.cuda.empty_cache()


def _load_pipeline(model: str, base_model: str | None) -> Any:
    from diffusers import (
        StableDiffusionPipeline,
        StableDiffusionXLPipeline,
    )

    resolved = _resolve_model_path(model)
    dtype = _dtype()
    device = _device()
    LOG.info("loading pipeline model=%s resolved=%s device=%s", model, resolved, device)

    common = {
        "torch_dtype": dtype,
        "safety_checker": None,
        "requires_safety_checker": False,
    }

    is_file = os.path.isfile(resolved)
    use_xl = _looks_xl(model, base_model) or (
        is_file and Path(resolved).stat().st_size > 4_000_000_000
    )

    if is_file:
        if use_xl and not _looks_sd15(model, base_model):
            pipe = StableDiffusionXLPipeline.from_single_file(resolved, **common)
        else:
            pipe = StableDiffusionPipeline.from_single_file(resolved, **common)
    else:
        if use_xl:
            pipe = StableDiffusionXLPipeline.from_pretrained(resolved, **common)
        else:
            pipe = StableDiffusionPipeline.from_pretrained(resolved, **common)

    pipe = pipe.to(device)
    if device == "cuda":
        try:
            pipe.enable_attention_slicing()
        except Exception:
            pass
    return pipe


def _seed_generator(seed: int) -> torch.Generator:
    """Manual seed on CPU — matches Automatic1111 / webui reproducibility conventions."""
    gen = torch.Generator(device="cpu")
    return gen.manual_seed(int(seed) & 0xFFFFFFFF)


def _lora_cache_key(lora_request: list[dict[str, Any]] | None) -> list[str]:
    """Model + ordered adapter identity including scale (VRAM cache key)."""
    if not lora_request:
        return []
    keys: list[str] = []
    for item in lora_request:
        name = (
            item.get("lora_name")
            or item.get("loraName")
            or item.get("name")
            or item.get("path")
        )
        if not name:
            continue
        scale = float(
            item.get("lora_scale")
            or item.get("loraScale")
            or item.get("strength")
            or item.get("scale")
            or item.get("weight")
            or 1.0
        )
        keys.append(f"{name}@{scale:.4f}")
    return keys


def _apply_loras(pipe: Any, lora_request: list[dict[str, Any]] | None) -> list[str]:
    names: list[str] = []
    if not lora_request:
        return names
    try:
        pipe.unload_lora_weights()
    except Exception:
        pass
    adapters: list[str] = []
    weights: list[float] = []
    for i, item in enumerate(lora_request):
        name = (
            item.get("lora_name")
            or item.get("loraName")
            or item.get("name")
            or item.get("path")
            or ""
        )
        if not name:
            continue
        path = _resolve_lora_path(str(name))
        scale = float(
            item.get("lora_scale")
            or item.get("loraScale")
            or item.get("strength")
            or item.get("scale")
            or item.get("weight")
            or 1.0
        )
        adapter = f"lora_{i}"
        LOG.info("load_lora %s -> %s scale=%s", name, path, scale)
        pipe.load_lora_weights(str(path.parent), weight_name=path.name, adapter_name=adapter)
        adapters.append(adapter)
        weights.append(scale)
        names.append(path.stem)
    if adapters:
        # Scaled PEFT adapters only — never fuse into weights (cached pipelines would stack).
        pipe.set_adapters(adapters, adapter_weights=weights)
    return names


def _ensure_pipe(
    model: str,
    base_model: str | None,
    lora_request: list[dict[str, Any]] | None,
) -> tuple[Any, list[str]]:
    global _pipe, _loaded_model, _loaded_loras
    want_loras = _lora_cache_key(lora_request)
    same = (
        _pipe is not None
        and _loaded_model == model
        and _loaded_loras == want_loras
    )
    if same:
        return _pipe, list(_loaded_loras)
    _unload_locked()
    pipe = _load_pipeline(model, base_model)
    applied = _apply_loras(pipe, lora_request)
    _pipe = pipe
    _loaded_model = model
    _loaded_loras = want_loras
    return pipe, applied


def _norm_sampler(name: str) -> str:
    s = " ".join(name.strip().lower().replace("_", " ").split())
    return s.replace("dpmpp", "dpm++")


def _resolve_sampler(name: str | None) -> tuple[str, str, str | None]:
    """Return (applied label, kind, note).

    kind is euler_a, euler, or dpm2m_karras. Exact for Euler a, Euler, and
    DPM++ 2M Karras. Other names use the closest of those and say so.
    """
    requested = (name or "").strip()
    key = _norm_sampler(requested)
    exact = {
        "euler a": "euler_a",
        "eulera": "euler_a",
        "euler ancestral": "euler_a",
        "euler ancestral discrete": "euler_a",
        "euler": "euler",
        "euler discrete": "euler",
        "dpm++ 2m karras": "dpm2m_karras",
        "dpm++2m karras": "dpm2m_karras",
    }
    kind = exact.get(key)
    matched = kind is not None
    if kind is None:
        if "euler" in key and ("ancestral" in key or key.endswith(" a")):
            kind = "euler_a"
        elif "euler" in key:
            kind = "euler"
        elif "dpm" in key or "karras" in key:
            kind = "dpm2m_karras"
        else:
            kind = "euler_a"
    applied = {
        "euler_a": "Euler a",
        "euler": "Euler",
        "dpm2m_karras": "DPM++ 2M Karras",
    }[kind]
    note = None
    if requested and not matched:
        sched = {
            "euler_a": "EulerAncestralDiscreteScheduler",
            "euler": "EulerDiscreteScheduler",
            "dpm2m_karras": "DPMSolverMultistepScheduler",
        }[kind]
        note = f"requested {requested}; applied {applied} ({sched})"
    return applied, kind, note


def _apply_scheduler(pipe: Any, sampler: str | None) -> tuple[str | None, str | None, str | None]:
    """Set a Diffusers scheduler. On failure, keep the pipeline default and record it."""
    if not sampler or not str(sampler).strip():
        return None, None, None
    applied, kind, note = _resolve_sampler(sampler)
    try:
        from diffusers import (
            DPMSolverMultistepScheduler,
            EulerAncestralDiscreteScheduler,
            EulerDiscreteScheduler,
        )

        config = dict(pipe.scheduler.config)
        if kind == "euler_a":
            config["timestep_spacing"] = "leading"
            pipe.scheduler = EulerAncestralDiscreteScheduler.from_config(config)
            sched = "EulerAncestralDiscreteScheduler"
        elif kind == "euler":
            pipe.scheduler = EulerDiscreteScheduler.from_config(config)
            sched = "EulerDiscreteScheduler"
        else:
            pipe.scheduler = DPMSolverMultistepScheduler.from_config(
                config,
                algorithm_type="dpmsolver++",
                use_karras_sigmas=True,
                solver_order=2,
            )
            sched = "DPMSolverMultistepScheduler"
        return applied, sched, note
    except Exception as e:
        LOG.warning("scheduler map failed for %s: %s", sampler, e)
        fallback = f"requested {sampler}; scheduler unchanged ({e})"
        return str(sampler).strip(), None, note or fallback


def _random_seed() -> int:
    return secrets.randbelow(2_147_483_646) or 1


def _strip_a1111_resource_tags(prompt: str) -> str:
    return _A1111_INLINE_TAG.sub("", prompt).rstrip()


def _sdxl_pipeline(pipe: Any) -> bool:
    return pipe.__class__.__name__ == "StableDiffusionXLPipeline"


def _needs_a1111_prompt_parse(prompt: str, negative: str | None) -> bool:
    blob = f"{prompt}\n{negative or ''}"
    return bool(_A1111_ATTENTION.search(blob))


def _compel_embeddings_type(clip_skip: int | None, sdxl: bool):
    from compel import ReturnedEmbeddingsType

    skip = max(1, int(clip_skip) if clip_skip is not None else 1)
    if skip > 1:
        if sdxl:
            return ReturnedEmbeddingsType.PENULTIMATE_HIDDEN_STATES_NON_NORMALIZED
        return ReturnedEmbeddingsType.PENULTIMATE_HIDDEN_STATES_NORMALIZED
    return ReturnedEmbeddingsType.LAST_HIDDEN_STATES_NORMALIZED


def _encode_a1111_prompts(
    pipe: Any, prompt: str, negative: str | None, clip_skip: int | None
) -> dict[str, Any]:
    """Encode with Compel so (word) and (word:1.2) match A1111-style conditioning."""
    from compel import Compel

    device = getattr(pipe, "device", _device())
    if _sdxl_pipeline(pipe):
        compel = Compel(
            tokenizer=[pipe.tokenizer, pipe.tokenizer_2],
            text_encoder=[pipe.text_encoder, pipe.text_encoder_2],
            returned_embeddings_type=_compel_embeddings_type(clip_skip, sdxl=True),
            requires_pooled=[False, True],
            truncate_long_prompts=False,
            device=device,
        )
        neg_text = negative or ""
        # Batch both prompts so Compel pads to the same sequence length (long prompt + short negative).
        cond, pooled = compel([prompt, neg_text])
        return {
            "prompt_embeds": cond[0:1],
            "pooled_prompt_embeds": pooled[0:1],
            "negative_prompt_embeds": cond[1:2],
            "negative_pooled_prompt_embeds": pooled[1:2],
        }
    compel = Compel(
        tokenizer=pipe.tokenizer,
        text_encoder=pipe.text_encoder,
        returned_embeddings_type=_compel_embeddings_type(clip_skip, sdxl=False),
        truncate_long_prompts=False,
        device=device,
    )
    out: dict[str, Any] = {"prompt_embeds": compel(prompt)}
    if negative:
        out["negative_prompt_embeds"] = compel(negative)
    return out


def _prompt_token_diag(pipe: Any, prompt: str, negative: str | None) -> dict[str, Any]:
    """Rough token counts for SDXL dual encoders (SDXL reports the longer path)."""
    out: dict[str, Any] = {"promptChars": len(prompt), "negativeChars": len(negative or "")}
    try:
        if _sdxl_pipeline(pipe):
            t1 = pipe.tokenizer(prompt, truncation=False, return_tensors="pt")
            t2 = pipe.tokenizer_2(prompt, truncation=False, return_tensors="pt")
            n1 = int(t1["input_ids"].shape[-1])
            n2 = int(t2["input_ids"].shape[-1])
            out["promptTokens"] = max(n1, n2)
            out["promptTokensClipL"] = n1
            out["promptTokensClipG"] = n2
            if negative:
                u1 = pipe.tokenizer(negative, truncation=False, return_tensors="pt")
                u2 = pipe.tokenizer_2(negative, truncation=False, return_tensors="pt")
                out["negativeTokens"] = max(int(u1["input_ids"].shape[-1]), int(u2["input_ids"].shape[-1]))
            # Compel path uses both encoders; flag when either encoder context is long.
            out["promptMayTruncate"] = n1 > 77 or n2 > 77
        else:
            t = pipe.tokenizer(prompt, truncation=False, return_tensors="pt")
            n = int(t["input_ids"].shape[-1])
            out["promptTokens"] = n
            out["promptMayTruncate"] = n > 77
            if negative:
                u = pipe.tokenizer(negative, truncation=False, return_tensors="pt")
                out["negativeTokens"] = int(u["input_ids"].shape[-1])
    except Exception as e:
        out["promptTokenError"] = str(e)
    return out


def _generation_prompt_kwargs(
    pipe: Any, prompt: str, negative: str | None, clip_skip: int | None
) -> dict[str, Any]:
    skip = int(clip_skip) if clip_skip is not None else 1
    use_compel = _sdxl_pipeline(pipe) or _needs_a1111_prompt_parse(prompt, negative) or skip > 1
    if use_compel:
        try:
            return _encode_a1111_prompts(pipe, prompt, negative, clip_skip)
        except Exception as e:
            LOG.warning("compel encode failed (%s); using raw prompt strings", e)
    kwargs: dict[str, Any] = {"prompt": prompt}
    if negative:
        kwargs["negative_prompt"] = negative
    return kwargs


def _call_pipe(pipe: Any, kwargs: dict[str, Any], clip_skip: int | None) -> tuple[Any, bool]:
    """Pass clip_skip when the pipeline accepts it. Never fail the generate over it."""
    if kwargs.get("prompt_embeds") is not None:
        skip = int(clip_skip) if clip_skip is not None else 0
        return pipe(**kwargs), skip > 1
    skip = int(clip_skip) if clip_skip is not None else 0
    if skip <= 1:
        return pipe(**kwargs), False
    trial = dict(kwargs)
    trial["clip_skip"] = skip
    try:
        out = pipe(**trial)
        if _sdxl_pipeline(pipe) and skip > 1:
            return out, True
        return out, True
    except TypeError as e:
        LOG.warning("clip_skip unsupported, stored only: %s", e)
    except Exception as e:
        msg = str(e).lower()
        if "clip_skip" in msg or "unexpected keyword" in msg:
            LOG.warning("clip_skip rejected, stored only: %s", e)
        else:
            raise
    return pipe(**kwargs), False


def _guidance(req: GenerateReq) -> float:
    if req.guidance_scale is not None:
        return float(req.guidance_scale)
    if req.cfg is not None:
        return float(req.cfg)
    return DEFAULT_GUIDANCE


@app.get("/health")
def health() -> dict[str, Any]:
    cuda = torch.cuda.is_available()
    vram = None
    if cuda:
        try:
            free, total = torch.cuda.mem_get_info()
            vram = {
                "usedMiB": int((total - free) / (1024 * 1024)),
                "totalMiB": int(total / (1024 * 1024)),
            }
        except Exception:
            vram = None
    return {
        "ok": True,
        "engine": "diffusers",
        "cuda": cuda,
        "device": _device(),
        "loadedInVram": _pipe is not None,
        "model": _loaded_model,
        "loras": list(_loaded_loras),
        "keepLoaded": KEEP_LOADED,
        "modelsDir": str(MODELS_DIR),
        "vram": vram,
    }


@app.post("/v1/unload")
def unload() -> dict[str, Any]:
    with _lock:
        _unload_locked()
    return {"ok": True, "loadedInVram": False}


@app.post("/v1/generate_gif")
def generate_gif(req: GenerateReq) -> JSONResponse:
    """Multi-frame SDXL animation → GIF (local motion path for gif modality)."""
    if not req.prompt.strip():
        raise HTTPException(status_code=400, detail="prompt is required")
    model = (req.model or "").strip()
    if not model:
        raise HTTPException(status_code=400, detail="model is required")
    frames = max(2, min(int(req.frames or 6), 12))
    width, height = _parse_size(req.size or "768x768")
    # Keep GIF frames modest for VRAM/time.
    width = min(width, 768)
    height = min(height, 768)
    steps = min(req.steps or 15, 20)
    guidance = _guidance(req)
    base_seed = int(req.seed if req.seed is not None else _random_seed()) & 0xFFFFFFFF
    device = _device()
    clip_skip = req.clip_skip

    with _lock:
        try:
            pipe, lora_names = _ensure_pipe(model, req.base_model, req.lora_request)
            applied, scheduler, sampler_note = _apply_scheduler(pipe, req.sampler)
            images = []
            clip_applied = False
            for i in range(frames):
                gen = _seed_generator((base_seed + i * 17) & 0xFFFFFFFF)
                kwargs: dict[str, Any] = {
                    "num_inference_steps": steps,
                    "guidance_scale": guidance,
                    "width": width,
                    "height": height,
                    "generator": gen,
                }
                kwargs.update(
                    _generation_prompt_kwargs(pipe, req.prompt, req.negative_prompt, clip_skip)
                )
                out, used_clip = _call_pipe(pipe, kwargs, clip_skip)
                clip_applied = clip_applied or used_clip
                images.append(out.images[0])
            buf = io.BytesIO()
            images[0].save(
                buf,
                format="GIF",
                save_all=True,
                append_images=images[1:],
                duration=120,
                loop=0,
            )
            b64 = base64.b64encode(buf.getvalue()).decode("ascii")
            if not KEEP_LOADED:
                _unload_locked()
            return JSONResponse(
                {
                    "data": [{"b64_json": b64, "seed": base_seed}],
                    "model": model,
                    "loras": lora_names,
                    "engine": "diffusers",
                    "format": "gif",
                    "frames": frames,
                    "stub": False,
                    "mime": "image/gif",
                    "seed": base_seed,
                    "steps": steps,
                    "guidance_scale": guidance,
                    "cfg": guidance,
                    "sampler": req.sampler or applied,
                    "scheduler": scheduler,
                    "samplerNote": sampler_note,
                    "clipSkip": clip_skip,
                    "clipSkipApplied": clip_applied,
                    "quantity": 1,
                    "width": width,
                    "height": height,
                }
            )
        except HTTPException:
            raise
        except Exception as e:
            LOG.exception("gif generate failed")
            _unload_locked()
            raise HTTPException(status_code=500, detail=f"gif generate failed: {e}") from e


@app.post("/v1/generate")
def generate(req: GenerateReq) -> JSONResponse:
    if not req.prompt.strip():
        raise HTTPException(status_code=400, detail="prompt is required")
    model = (req.model or "").strip()
    if not model:
        raise HTTPException(status_code=400, detail="model is required")

    prompt = req.prompt.strip()
    if req.lora_request:
        cleaned = _strip_a1111_resource_tags(prompt)
        if cleaned != prompt:
            LOG.info(
                "stripped inline lora tags from prompt (%d -> %d chars)",
                len(prompt),
                len(cleaned),
            )
            prompt = cleaned

    width, height = _parse_size(req.size)
    steps = max(1, min(int(req.steps or DEFAULT_STEPS), 80))
    guidance = _guidance(req)
    quantity = max(1, min(int(req.quantity or req.n or 1), 4))
    base_seed = int(req.seed if req.seed is not None else _random_seed()) & 0xFFFFFFFF
    device = _device()
    clip_skip = req.clip_skip

    with _lock:
        try:
            pipe, lora_names = _ensure_pipe(model, req.base_model, req.lora_request)
            applied, scheduler, sampler_note = _apply_scheduler(pipe, req.sampler)
            data = []
            clip_applied = False
            for i in range(quantity):
                seed_i = (base_seed + i) & 0xFFFFFFFF
                generator = _seed_generator(seed_i)
                kwargs: dict[str, Any] = {
                    "num_inference_steps": steps,
                    "guidance_scale": guidance,
                    "width": width,
                    "height": height,
                    "generator": generator,
                }
                kwargs.update(
                    _generation_prompt_kwargs(pipe, prompt, req.negative_prompt, clip_skip)
                )
                LOG.info(
                    "generate model=%s size=%sx%s steps=%s guidance=%s sampler=%s clip_skip=%s seed=%s i=%s loras=%s",
                    model,
                    width,
                    height,
                    steps,
                    guidance,
                    applied or req.sampler,
                    clip_skip,
                    seed_i,
                    i,
                    lora_names,
                )
                out, used_clip = _call_pipe(pipe, kwargs, clip_skip)
                clip_applied = clip_applied or used_clip
                image = out.images[0]
                buf = io.BytesIO()
                image.save(buf, format="PNG")
                b64 = base64.b64encode(buf.getvalue()).decode("ascii")
                data.append({"b64_json": b64, "seed": seed_i})
            prompt_diag = _prompt_token_diag(pipe, prompt, req.negative_prompt)
            if not KEEP_LOADED:
                _unload_locked()
            return JSONResponse(
                {
                    "data": data,
                    "model": model,
                    "loras": lora_names,
                    "engine": "diffusers",
                    "stub": False,
                    "width": width,
                    "height": height,
                    "steps": steps,
                    "guidance_scale": guidance,
                    "cfg": guidance,
                    "sampler": req.sampler or applied,
                    "scheduler": scheduler,
                    "samplerNote": sampler_note,
                    "clipSkip": clip_skip,
                    "clipSkipApplied": clip_applied,
                    "seed": base_seed,
                    "quantity": quantity,
                    "negative_prompt": req.negative_prompt,
                    "workerPrompt": prompt if prompt != req.prompt.strip() else None,
                    "promptEncoding": "compel-sdxl" if _sdxl_pipeline(pipe) else "compel-sd15",
                    **prompt_diag,
                }
            )
        except HTTPException:
            raise
        except Exception as e:
            LOG.exception("generate failed")
            _unload_locked()
            raise HTTPException(status_code=500, detail=f"generate failed: {e}") from e


def main() -> None:
    logging.basicConfig(
        level=os.environ.get("FREYA_ENGINE_LOG", "INFO"),
        format="%(asctime)s %(levelname)s %(name)s %(message)s",
    )
    host = os.environ.get("FREYA_ENGINE_HOST", "127.0.0.1")
    port = int(os.environ.get("FREYA_ENGINE_PORT", "8092"))
    LOG.info(
        "freya diffusers engine listening on %s:%s models=%s cuda=%s keep_loaded=%s",
        host,
        port,
        MODELS_DIR,
        torch.cuda.is_available(),
        KEEP_LOADED,
    )
    if torch.cuda.is_available():
        LOG.info("GPU: %s", torch.cuda.get_device_name(0))
    uvicorn.run(app, host=host, port=port, log_level="info")


if __name__ == "__main__":
    main()
