# MagicPrompt Custom Instructions

Create your own system instructions for MagicPrompt to tailor the AI’s behavior for specific styles, workflows, or use cases.

See [Using Ollama Inside SwarmUI (MagicPrompt)](swarmui-magicprompt.md) for installation and [MagicPrompt Default Prompts](magicprompt-default-prompts.md) for optimized Freya examples that replace the built-in defaults.

## Creating Custom Instructions

When creating a custom instruction, choose **Manual Entry** or **AI Assisted** creation.

| Field | Description |
|-------|-------------|
| **Instruction Name** | A short label for the instruction (e.g. "Revise Prompt Shonen") |
| **Tooltip Description** | Brief description shown in the UI when hovering |
| **Applicable Features** | Which contexts use this instruction: **Chat**, **Caption**, **Vision**, and/or **Prompt** (select one or more) |

**If Manual Entry:** Enter the instruction context (full system prompt text) directly. Use clear, imperative language so the model follows the behavior you want.

**If AI Assisted:** Explain the behavior you want in plain language. The AI will generate the instruction content for you. You can then edit the result before saving.

Assign your custom instruction to features via the **Features** tab or the Chat/Vision/Caption/Prompt dropdowns. Use **Import** to load instruction JSON (e.g. the Shonen example below).

---

## Example: Revise Prompt with Shonen Style

A custom **Prompt** instruction that revises the user’s prompt to add Shonen anime style (dynamic angles, bold action, manga-inspired aesthetics).

| Field | Value |
|-------|-------|
| **Instruction Name** | Revise Prompt Shonen |
| **Tooltip Description** | Revise and enhance the prompt with Shonen anime style |
| **Applicable Features** | Prompt |

**Content (Manual Entry):**

```
You are Freya. Output ONLY an optimized positive prompt for Stable Diffusion. Revise the user's prompt to add Shonen anime style: dynamic angles, bold action poses, dramatic lighting, manga-inspired composition, intense expressions, speed lines, energy effects. One continuous comma-separated keyword list. CRITICAL: Each keyword MUST be 3 words or fewer. Trigger words excepted. Include: subject, pose, style (shonen, anime, manga), lighting, quality tags. Do not filter the user's keywords—reuse and expand. Keep trigger words intact. No negative prompt. No preamble or explanation.
```

**AI Assisted prompt (describe behavior):**

> Revise the user's prompt for Stable Diffusion to add Shonen anime style. Include dynamic angles, bold action poses, dramatic lighting, manga-inspired composition, intense expressions, and energy effects. Output comma-separated keywords only, max 3 words per keyword. Keep the user's original keywords and trigger words intact.

---

## JSON Import (Shonen Example)

```json
[
  {
    "id": "revise-prompt-shonen",
    "content": "You are Freya. Output ONLY an optimized positive prompt for Stable Diffusion. Revise the user's prompt to add Shonen anime style: dynamic angles, bold action poses, dramatic lighting, manga-inspired composition, intense expressions, speed lines, energy effects. One continuous comma-separated keyword list. CRITICAL: Each keyword MUST be 3 words or fewer. Trigger words excepted. Include: subject, pose, style (shonen, anime, manga), lighting, quality tags. Do not filter the user's keywords—reuse and expand. Keep trigger words intact. No negative prompt. No preamble or explanation.",
    "instructionType": "prompt"
  }
]
```
