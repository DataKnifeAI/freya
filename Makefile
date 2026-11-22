IMG ?= harbor.dataknife.net/library/freya-operator:0.1.0
API_IMG ?= harbor.dataknife.net/library/freya-api:0.1.0
WEB_IMG ?= harbor.dataknife.net/library/freya-web:0.1.0
WORKER_IMG ?= harbor.dataknife.net/library/freya-diffusion-worker:0.1.0

VERSION ?= 0.1.0
CARGO ?= cargo

.PHONY: all help build release test fmt fmt-check lint vet ci run-api run-worker run-operator \
	manifests install deploy up-local kind-hosts kind-tls clean docker-build

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
	$(CARGO) run -p freya-operator --bin freya-crdgen > config/crd/freya.dataknife.ai_crds.yaml

install: manifests ## Apply CRDs
	kubectl apply -f config/crd/freya.dataknife.ai_crds.yaml
	kubectl apply -f config/rbac/

deploy: ## Deploy operator + sample studio (edit secrets first)
	kubectl apply -k config/default

run-api: ## Run Freya API locally
	$(CARGO) run -p freya-api -- --bind 127.0.0.1:8080

run-worker: ## Run diffusion worker stub locally
	$(CARGO) run -p freya-worker -- --port 8091

run-operator: ## Run operator (needs kubeconfig)
	$(CARGO) run -p freya-operator --bin freya-operator

up-local: ## API + notes for UI (copy .env.example → .env first)
	@echo "cp .env.example .env  # fill OPENROUTER_API_KEY (required)"
	@echo "set -a && source .env && set +a"
	@echo "Terminal A: make run-api"
	@echo "Terminal B: cd web && npm install && npm run dev"
	@echo "UI mock only:  make mock-ui"
	@echo "Optional worker: make run-worker  then Settings → http://127.0.0.1:8091"
	@echo "Optional: CIVITAI_API_KEY, FREYA_API_TOKEN, FREYA_WORKER_TOKEN, FREYA_CORS_ORIGINS"

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
	docker build -f Dockerfile.web -t $(WEB_IMG) .
