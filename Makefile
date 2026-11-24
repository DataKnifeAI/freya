IMG ?= harbor.dataknife.net/library/freya-operator:0.1.0
API_IMG ?= harbor.dataknife.net/library/freya-api:0.1.0
WEB_IMG ?= harbor.dataknife.net/library/freya-web:0.1.0
WORKER_IMG ?= harbor.dataknife.net/library/freya-diffusion-worker:0.1.0
MCP_IMG ?= harbor.dataknife.net/library/freya-mcp:0.1.0

VERSION ?= 0.1.0
CARGO ?= cargo

.PHONY: all help build release test fmt fmt-check lint vet ci run-api run-worker run-operator \
	run-mcp manifests install deploy up-local kind-hosts kind-tls clean docker-build \
	kind-load-mcp kind-bin-mcp kind-deploy-mcp kind-bin-api kind-bin-web kind-deploy-api-web \
	sync-api-token compose-up compose-up-gpu compose-up-mcp compose-down compose-logs compose-ps

all: build

help: ## Show this help
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / {printf "  %-18s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

build: ## Build workspace
	$(CARGO) build --workspace

release: ## Build release binaries
	$(CARGO) build --workspace --release

test: ## Run tests
	$(CARGO) test --workspace

fmt: ## Format Rust sources
	$(CARGO) fmt --all

fmt-check: ## Check formatting (CI)
	$(CARGO) fmt --all -- --check

lint: ## Clippy with warnings denied
	$(CARGO) clippy --workspace --all-targets -- -D warnings

vet: fmt-check lint ## Static checks

ci: vet test ## Local quality gate

manifests: ## Generate CRD YAML into config/crd
	bash -o pipefail -c '$(CARGO) run -q -p freya-operator --bin freya-crdgen | awk "BEGIN{p=0} /^apiVersion:/{p=1} p" > config/crd/freya.dataknife.ai_crds.yaml.tmp && test -s config/crd/freya.dataknife.ai_crds.yaml.tmp && mv config/crd/freya.dataknife.ai_crds.yaml.tmp config/crd/freya.dataknife.ai_crds.yaml || { rm -f config/crd/freya.dataknife.ai_crds.yaml.tmp; exit 1; }'

install: manifests ## Apply CRDs
	kubectl apply -f config/crd/freya.dataknife.ai_crds.yaml
	kubectl apply -f config/rbac/

deploy: ## Deploy operator + sample studio (edit secrets first)
	kubectl apply -k config/default

run-api: ## Run Freya API locally
	$(CARGO) run -p freya-api -- --bind 127.0.0.1:8080

run-worker: ## Run diffusion worker locally (stub PNG unless FREYA_ENGINE_URL set)
	$(CARGO) run -p freya-worker -- --port 8091

.PHONY: run-local-worker
run-local-worker: ## Build/run Docker freya-local-worker with GPU + Diffusers
	./scripts/run-local-worker.sh

.PHONY: update-local-worker
update-local-worker: ## Pull CR/image tag and recreate freya-local-worker (GPU host)
	./scripts/update-local-worker.sh

run-operator: ## Run operator (needs kubeconfig)
	$(CARGO) run -p freya-operator --bin freya-operator

run-mcp: ## Run Freya MCP Streamable HTTP locally (:8090/mcp)
	$(CARGO) run -p freya-mcp -- --bind 127.0.0.1:8090 --api-url http://127.0.0.1:8080

up-local: ## API + notes for UI (copy .env.example → .env first)
	@echo "cp .env.example .env  # fill OPENROUTER_API_KEY (required)"
	@echo "set -a && source .env && set +a"
	@echo "Terminal A: make run-api"
	@echo "Terminal B: cd web && npm install && npm run dev"
	@echo "Optional MCP: make run-mcp  # http://127.0.0.1:8090/mcp"
	@echo "UI mock only:  make mock-ui"
	@echo "Optional worker: make run-worker  then Settings → http://127.0.0.1:8091"
	@echo "Optional: CIVITAI_API_KEY, FREYA_API_TOKEN, FREYA_WORKER_TOKEN, FREYA_CORS_ORIGINS"
	@echo "Single-system Docker (no k8s): make compose-up  # see docs/COMPOSE.md"

# --- Docker Compose single-system (no k8s / operator; distinct from legacy/) ---
COMPOSE_FILE ?= deploy/compose/compose.yaml
COMPOSE_GPU_FILE ?= deploy/compose/compose.gpu.yaml
COMPOSE_ENV ?= deploy/compose/.env
COMPOSE := docker compose -f $(COMPOSE_FILE) --env-file $(COMPOSE_ENV)

compose-up: ## Single-system Freya via Compose (api+web+worker)
	@test -f $(COMPOSE_ENV) || (echo "Copy deploy/compose/.env.example → $(COMPOSE_ENV) and set keys"; exit 1)
	$(COMPOSE) up -d --build
	@echo "UI http://127.0.0.1:$${FREYA_UI_PORT:-8080}  worker :$${FREYA_WORKER_PORT:-8091}"
	@echo "Docs: docs/COMPOSE.md"

compose-up-gpu: ## Compose + NVIDIA GPU for freya-worker
	@test -f $(COMPOSE_ENV) || (echo "Copy deploy/compose/.env.example → $(COMPOSE_ENV) and set keys"; exit 1)
	docker compose -f $(COMPOSE_FILE) -f $(COMPOSE_GPU_FILE) --env-file $(COMPOSE_ENV) up -d --build
	@echo "UI http://127.0.0.1:$${FREYA_UI_PORT:-8080}  worker :$${FREYA_WORKER_PORT:-8091} (GPU)"

compose-up-mcp: ## Compose + optional freya-mcp (:8090)
	@test -f $(COMPOSE_ENV) || (echo "Copy deploy/compose/.env.example → $(COMPOSE_ENV) and set keys"; exit 1)
	COMPOSE_PROFILES=mcp $(COMPOSE) up -d --build
	@echo "UI :8080  worker :8091  MCP http://127.0.0.1:$${FREYA_MCP_PORT:-8090}/mcp"

compose-down: ## Stop Compose stack
	@# No --env-file required for teardown; defaults in compose.yaml apply.
	-docker compose -f $(COMPOSE_FILE) -f $(COMPOSE_GPU_FILE) --profile mcp down

compose-logs: ## Follow Compose logs
	$(COMPOSE) logs -f

compose-ps: ## Compose service status
	$(COMPOSE) ps

.PHONY: mock-ui
mock-ui: ## Interactive Freya UI mock (no API / keys required)
	cd web && npm install && npm run dev:mock

kind-hosts: ## Map freya.dataknife.net → 127.0.0.1 for kind Ingress
	./scripts/kind-hosts.sh

kind-tls: ## Sync LE *.dataknife.net wildcard into kind + nginx default cert
	./scripts/kind-sync-wildcard-tls.sh

clean:
	$(CARGO) clean

docker-build: ## Build container images
	docker build -f Dockerfile -t $(IMG) --target operator .
	docker build -f Dockerfile -t $(API_IMG) --target api .
	docker build -f Dockerfile -t $(WORKER_IMG) --target worker .
	docker build -f Dockerfile -t $(MCP_IMG) --target mcp .
	docker build -f Dockerfile.web -t $(WEB_IMG) .

KIND_CLUSTER ?= freya

kind-load-mcp: ## Full Docker multi-stage build of operator+mcp → kind
	docker build -f Dockerfile -t $(IMG) --target operator .
	docker build -f Dockerfile -t $(MCP_IMG) --target mcp .
	kind load docker-image $(IMG) --name $(KIND_CLUSTER)
	kind load docker-image $(MCP_IMG) --name $(KIND_CLUSTER)

# Fast kind path: host release binaries → distroless images (Dockerfile.bin).
kind-bin-mcp: ## cargo release + load operator/mcp images into kind
	# freya-api binary required in context: legacy docker builds prior Dockerfile.bin stages.
	$(CARGO) build --release -p freya-operator -p freya-mcp -p freya-api
	@rm -rf /tmp/freya-bin-ctx && mkdir -p /tmp/freya-bin-ctx/target/release /tmp/freya-bin-ctx/packs
	cp target/release/freya-operator target/release/freya-mcp target/release/freya-api /tmp/freya-bin-ctx/target/release/
	cp -a packs/. /tmp/freya-bin-ctx/packs/
	cp Dockerfile.bin /tmp/freya-bin-ctx/Dockerfile
	docker build -f /tmp/freya-bin-ctx/Dockerfile -t $(IMG) --target operator /tmp/freya-bin-ctx
	docker build -f /tmp/freya-bin-ctx/Dockerfile -t $(MCP_IMG) --target mcp /tmp/freya-bin-ctx
	kind load docker-image $(IMG) --name $(KIND_CLUSTER)
	kind load docker-image $(MCP_IMG) --name $(KIND_CLUSTER)

kind-deploy-mcp: kind-bin-mcp manifests install ## Fast load + apply CRDs + restart operator
	kubectl -n freya-system set image deploy/freya-operator manager=$(IMG)
	kubectl -n freya-system rollout restart deploy/freya-operator
	kubectl -n freya-system rollout status deploy/freya-operator --timeout=120s
	@echo "MCP: https://freya.dataknife.net/mcp  (after Studio reconcile)"
	@echo "In-cluster: kubectl -n freya get freyastudio freya -o jsonpath='{.status.mcpEndpoint}'"

kind-bin-api: ## cargo release freya-api → kind
	$(CARGO) build --release -p freya-api -p freya-operator
	@rm -rf /tmp/freya-bin-api && mkdir -p /tmp/freya-bin-api/target/release
	# Legacy docker builder evaluates earlier Dockerfile.bin stages — include operator bin too.
	cp target/release/freya-api target/release/freya-operator /tmp/freya-bin-api/target/release/
	cp -a packs /tmp/freya-bin-api/packs
	cp Dockerfile.bin /tmp/freya-bin-api/Dockerfile
	docker build -f /tmp/freya-bin-api/Dockerfile -t $(API_IMG) --target api /tmp/freya-bin-api
	kind load docker-image $(API_IMG) --name $(KIND_CLUSTER)

kind-bin-web: ## Build freya-web image → kind
	docker build -f Dockerfile.web -t $(WEB_IMG) .
	kind load docker-image $(WEB_IMG) --name $(KIND_CLUSTER)

kind-deploy-api-web: kind-bin-api kind-bin-web ## Load api+web into kind and restart Studio deploys
	kubectl -n freya set image deploy/freya-api api=$(API_IMG)
	kubectl -n freya set image deploy/freya-ui ui=$(WEB_IMG)
	kubectl -n freya rollout restart deploy/freya-api deploy/freya-ui
	kubectl -n freya rollout status deploy/freya-api --timeout=180s
	kubectl -n freya rollout status deploy/freya-ui --timeout=120s

sync-api-token: ## Align freya-api-token Secret with gallery PVC runtime token
	./scripts/sync-api-token-from-pvc.sh
