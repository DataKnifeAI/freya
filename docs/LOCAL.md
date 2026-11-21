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

### API auth (non-dev)

By default `FREYA_API_TOKEN` is empty and mutating `/v1` routes are open — freya-api logs a warning (local/dev). For anything exposed beyond localhost:

```bash
export FREYA_API_TOKEN='long-random-secret'
# optional: restrict browser origins (comma-separated)
export FREYA_CORS_ORIGINS='https://freya.example.com,http://127.0.0.1:5173'
```

Send `Authorization: Bearer $FREYA_API_TOKEN` on POST/PUT. GET gallery/image stays open so the UI can load artifacts.

`make ci` runs fmt, clippy, and tests.
