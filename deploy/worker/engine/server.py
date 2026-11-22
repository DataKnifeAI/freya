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


def _apply_loras(pipe: Any, lora_request: list[dict[str, Any]] | None) -> list[str]:
    names: list[str] = []
    if not lora_request:
        return names
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
        scale = float(item.get("strength") or item.get("scale") or item.get("weight") or 1.0)
        adapter = f"lora_{i}"
        LOG.info("load_lora %s -> %s scale=%s", name, path, scale)
        pipe.load_lora_weights(str(path.parent), weight_name=path.name, adapter_name=adapter)
        adapters.append(adapter)
        weights.append(scale)
        names.append(path.stem)
    if adapters:
        pipe.set_adapters(adapters, adapter_weights=weights)
    return names


def _ensure_pipe(
    model: str,
    base_model: str | None,
    lora_request: list[dict[str, Any]] | None,
) -> tuple[Any, list[str]]:
    global _pipe, _loaded_model, _loaded_loras
    want_loras = []
    if lora_request:
        for item in lora_request:
            n = item.get("lora_name") or item.get("loraName") or item.get("name") or item.get("path")
            if n:
                want_loras.append(str(n))
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
    guidance = req.guidance_scale if req.guidance_scale is not None else DEFAULT_GUIDANCE
    base_seed = int(req.seed if req.seed is not None else 42) & 0xFFFFFFFF
    device = _device()

    with _lock:
        try:
            pipe, lora_names = _ensure_pipe(model, req.base_model, req.lora_request)
            images = []
            for i in range(frames):
                gen = torch.Generator(device=device).manual_seed((base_seed + i * 17) & 0xFFFFFFFF)
                kwargs: dict[str, Any] = {
                    "prompt": req.prompt,
                    "num_inference_steps": steps,
                    "guidance_scale": guidance,
                    "width": width,
                    "height": height,
                    "generator": gen,
                }
                if req.negative_prompt:
                    kwargs["negative_prompt"] = req.negative_prompt
                out = pipe(**kwargs)
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
                    "data": [{"b64_json": b64}],
                    "model": model,
                    "loras": lora_names,
                    "engine": "diffusers",
                    "format": "gif",
                    "frames": frames,
                    "stub": False,
                    "mime": "image/gif",
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

    width, height = _parse_size(req.size)
    steps = req.steps or DEFAULT_STEPS
    guidance = req.guidance_scale if req.guidance_scale is not None else DEFAULT_GUIDANCE
    seed = req.seed
    device = _device()
    generator = None
    if seed is not None:
        generator = torch.Generator(device=device).manual_seed(int(seed) & 0xFFFFFFFF)

    with _lock:
        try:
            pipe, lora_names = _ensure_pipe(model, req.base_model, req.lora_request)
            kwargs: dict[str, Any] = {
                "prompt": req.prompt,
                "num_inference_steps": steps,
                "guidance_scale": guidance,
                "width": width,
                "height": height,
            }
            if req.negative_prompt:
                kwargs["negative_prompt"] = req.negative_prompt
            if generator is not None:
                kwargs["generator"] = generator
            LOG.info(
                "generate model=%s size=%sx%s steps=%s loras=%s",
                model,
                width,
                height,
                steps,
                lora_names,
            )
            out = pipe(**kwargs)
            image = out.images[0]
            buf = io.BytesIO()
            image.save(buf, format="PNG")
            b64 = base64.b64encode(buf.getvalue()).decode("ascii")
            if not KEEP_LOADED:
                _unload_locked()
            return JSONResponse(
                {
                    "data": [{"b64_json": b64}],
                    "model": model,
                    "loras": lora_names,
                    "engine": "diffusers",
                    "stub": False,
                    "width": width,
                    "height": height,
                    "steps": steps,
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
