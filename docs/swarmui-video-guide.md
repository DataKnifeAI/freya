# SwarmUI Video Generation — Beginner’s Guide

A step-by-step guide to text-to-video (T2V) and image-to-video (I2V) with SwarmUI is maintained on Hugging Face:

**[Beginner’s Guide — Generate Videos With SwarmUI](https://huggingface.co/blog/MonsterMMORPG/beginners-guide-generate-videos-with-swarmui)**

It walks through:

1. **Choosing video models** — Wan 2.1 (e.g. Text2Video 14B fp8_scaled, Image2Video 14B 480p fp8_scaled for RTX 4090), where to put them (`diffusion_models`, subfolders optional), and refreshing the model list in Swarm.
2. **Text-to-video** — Picking a T2V model, resetting params, setting CFG, frame count, resolution, and format, then generating from a prompt.
3. **Text-to-image-to-video** — Generating an image (e.g. with Flux), then enabling the Image To Video group, selecting the I2V model, and generating a video from that image.
4. **Direct image-to-video** — Using **Init Image** with **Init Image Creativity** set to **0**, choosing the I2V model, and prompting only the motion. The guide stresses keeping creativity at 0 when using an init image.
5. **Going further** — Other model classes, high-res/longer gens, performance tweaks, and batch I2V via Image Edit Batcher.

For supported model classes, paths, and options, use SwarmUI’s [Video Model Support](https://github.com/mcmonkeyprojects/SwarmUI/blob/master/docs/Video%20Model%20Support.md) doc. In Freya, SwarmUI model roots live under `swarmui/data/Models/` (see [MODELS.md](MODELS.md)).

## Recommended settings (Wan 2.1 14B fp8, RTX 4090)

From the [Hugging Face beginner’s guide](https://huggingface.co/blog/MonsterMMORPG/beginners-guide-generate-videos-with-swarmui); use **Quick Tools → Reset Params to Default** first, then adjust.

| Setting | T2V (text-to-video) | I2V (image-to-video) |
|--------|----------------------|------------------------|
| **CFG** | **6** | **6** (Wan default; leave Video CFG unchecked to use class default) |
| **Frames** | **49** (faster); default Wan = 81 for longer clips. Use a value your model supports (see Video Model Support doc). | **33** for quick gens; increase for longer clips. |
| **Steps** | Leave at model default after reset. | Leave at model default. |
| **Resolution** | **640×640** for speed; **960×960** for quality (Wan 14B default). | **512×512** or **640×640** for speed; set Video Resolution to “Image” to match init image. |
| **Format** | **webp** (best compatibility); **gif-hd** for GitHub/embedding. | **webp**; **gif-hd** for sharing. |

- Lower resolution and fewer frames = faster and less VRAM; higher = better quality, slower, more VRAM.
- For I2V with Init Image: set **Init Image Creativity** to **0** and prompt only the motion.
