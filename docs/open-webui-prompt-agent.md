# Open WebUI as Stable Diffusion Prompt Generator Agent

Open WebUI runs alongside Ollama to provide a chat interface that can act as a **Stable Diffusion prompt generator**. Users talk to an assistant that is instructed to produce concise, well-structured prompts suitable for ComfyUI or SwarmUI.

## Setup

1. **Start services** (Ollama and Open WebUI must be running):
   ```bash
   make up
   ```

2. **Pull an Ollama model** (if you haven’t already):
   ```bash
   make setup-ollama MODEL=llama3.2:1b
   ```
   Or use another model, e.g. `llama3.2:3b`, `mistral:7b`, etc.

3. **Open Open WebUI**: http://localhost:8080

4. **Sign up or log in** (first user becomes admin).

## Using It as a Prompt Generator

### Option A: Create an Assistant (recommended)

1. In Open WebUI, go to **Workspace** → **Assistants** (or **Create** → **Assistant**).
2. Create a new assistant, e.g. “SD Prompt Generator”.
3. Set the **Model** to an Ollama model you’ve pulled (e.g. `llama3.2:1b`).
4. Set the **System prompt** to something like:

   ```text
   You are a creative prompt generator for Stable Diffusion. Given a short idea or theme from the user, respond with a single, detailed image prompt suitable for AI image generation. Use clear, descriptive wording: subject, style, lighting, composition, quality terms (e.g. "masterpiece, best quality"). Output only the prompt, no extra explanation, unless the user asks for variations or reasoning.
   ```

5. Save the assistant and start a new chat with it. Describe what you want (e.g. “cyberpunk street at night”) and use the model’s output as the prompt in ComfyUI or SwarmUI.

### Option B: Ad‑hoc in a normal chat

In any chat, choose an Ollama model and tell the model in the first message:

- “You are a Stable Diffusion prompt writer. I’ll give themes; you reply with one image prompt per message.”

Then send your themes and paste the replies into your SD workflow.

## Connection to Ollama

Open WebUI is configured with `OLLAMA_BASE_URL=http://localhost:11434`, so it uses the Ollama service from the same stack. No extra configuration is needed when running with `make up` and host networking.

## Data location

Open WebUI stores its data under `openwebui/` (gitignored). User accounts, assistants, and chat history persist there between restarts.
