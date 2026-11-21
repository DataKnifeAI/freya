# Local development

## Interactive UI mock (no API)

Iterate on Freya UI without OpenRouter, Civitai, or a worker:

```bash
make mock-ui
# or: cd web && npm i && npm run dev:mock
```

Also: open the normal UI with `?mock=1`, or click **Remember mock** in the banner (stores `localStorage.freya.mock=1`).

Mock covers Generate, Enhance, Styles, Civitai search/install flow, Gallery remix, and Settings — in-memory with SVG placeholders.

## Full local stack

```bash
cp .env.example .env   # fill keys
export $(grep -v '^#' .env | xargs)

make run-api
# other terminal
cd web && npm i && npm run dev

# optional
make run-worker
# Settings → worker URL http://127.0.0.1:8091
# Settings → paste Civitai API key → Civitai tab → Install to worker
```

`make ci` runs fmt, clippy, and tests.
