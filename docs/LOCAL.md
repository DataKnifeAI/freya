# Local development

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
