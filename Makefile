.PHONY: help check-deps fix-docker-group build build-comfyui build-swarmui up down sui sui-rebuild swarmui-update cui llm llm-pull llm-import-hf llm-list llm-rm llm-logs swarmui-rebuild swarmui-extensions-check restart logs clean

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

sui: ## SwarmUI + Ollama (stops ComfyUI first; updates SwarmUI; preserves running containers)
	-docker compose stop comfyui 2>/dev/null || true
	docker compose up -d swarmui ollama
	@$(MAKE) swarmui-update

swarmui-update: ## Pull latest SwarmUI, rebuild if needed, and restart (requires swarmui container)
	@docker ps >/dev/null 2>&1 || (echo "Error: Docker not accessible. Run: make fix-docker-group, then newgrp docker" && exit 1)
	@docker compose up -d swarmui 2>/dev/null || true
	@i=0; while [ $$i -lt 90 ]; do \
		status=$$(docker compose ps swarmui --format '{{.Health}}' 2>/dev/null); \
		echo "$$status" | grep -q healthy && break; \
		docker compose ps swarmui --status running -q 2>/dev/null | grep -q . || { sleep 1; i=$$((i+1)); continue; }; \
		[ -z "$$status" ] && break; \
		i=$$((i+1)); sleep 1; \
	done
	@docker compose ps swarmui --status running -q 2>/dev/null | grep -q . \
		|| (echo "Error: SwarmUI not running. Start with: docker compose up -d swarmui" && exit 1)
	@echo "Updating SwarmUI (git pull + dotnet build if needed)..."
	@docker compose exec -T swarmui bash -c 'set -e; cd /app/SwarmUI; \
		prior=$$(git rev-parse HEAD); \
		for attempt in 1 2 3 4 5; do \
			if git fetch origin master && git reset --hard origin/master; then break; fi; \
			[ "$$attempt" -eq 5 ] && exit 1; \
			echo "Git fetch failed (attempt $$attempt), retrying..."; sleep 2; \
		done; \
		new=$$(git rev-parse HEAD); \
		rm -f src/bin/must_rebuild; \
		if [ "$$prior" != "$$new" ] || [ ! -f src/bin/live_release/SwarmUI.dll ]; then \
			echo "Building SwarmUI ($$prior -> $$new)..."; \
			dotnet build src/SwarmUI.csproj -c Release -o src/bin/live_release; \
			touch src/bin/.swarmui_restarted; \
		else \
			echo "SwarmUI source already up to date; skipping build."; \
		fi'
	@if docker compose exec -T swarmui test -f /app/SwarmUI/src/bin/.swarmui_restarted 2>/dev/null; then \
		docker compose exec -T swarmui rm -f /app/SwarmUI/src/bin/.swarmui_restarted; \
		docker compose restart swarmui; \
	fi

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

logs-swarmui: ## Show logs from SwarmUI service
	docker compose logs -f swarmui

llm-logs: ## Show logs from Ollama service
	docker compose logs -f ollama

clean: ## Remove containers, volumes, and images
	docker compose down -v

ps: ## Show running containers
	docker compose ps

shell-comfyui: ## Open shell in ComfyUI container
	docker compose exec comfyui /bin/bash

shell-swarmui: ## Open shell in SwarmUI container
	docker compose exec swarmui /bin/bash

swarmui-rebuild: ## [Debug] Force rebuild inside container without git pull. Prefer: make swarmui-update
	@docker ps >/dev/null 2>&1 || (echo "Error: Docker not accessible. Run: make fix-docker-group, then newgrp docker" && exit 1)
	@docker compose ps swarmui 2>/dev/null | grep -q Up || (echo "Error: SwarmUI not running. Start with: make sui" && exit 1)
	docker compose exec swarmui bash -c "set -e; cd /app/SwarmUI && dotnet build src/SwarmUI.csproj -c Release -o src/bin/live_release"
	docker compose restart swarmui

swarmui-extensions-check: ## [Debug] List extensions dir and run build; use if extension not showing after restart
	@docker ps >/dev/null 2>&1 || (echo "Error: Docker not accessible. Run: make fix-docker-group, then newgrp docker" && exit 1)
	@docker compose ps swarmui 2>/dev/null | grep -q Up || (echo "Error: SwarmUI not running. Start with: make sui" && exit 1)
	@echo "=== Extension dir (host: swarmui/extensions) ==="
	@docker compose exec swarmui ls -la /app/SwarmUI/src/Extensions/ 2>/dev/null || true
	@echo "=== Extension subdirs (.cs) ==="
	@docker compose exec swarmui find /app/SwarmUI/src/Extensions -maxdepth 2 -type f -name "*.cs" 2>/dev/null || true
	@echo "=== Manual build (SwarmUI usually builds at startup) ==="
	docker compose exec swarmui bash -c "set -e; cd /app/SwarmUI && dotnet build src/SwarmUI.csproj -c Release -o src/bin/live_release -v minimal"

llm-pull: ## Download Ollama model (usage: make llm-pull MODEL=dolphin3)
	@if [ -z "$(MODEL)" ]; then \
		echo "Usage: make llm-pull MODEL=<model_name>"; \
		echo "Example: make llm-pull MODEL=dolphin3  (default for MagicPrompt: https://ollama.com/library/dolphin3)"; \
		echo "Browse available models: https://ollama.com/library"; \
		echo "Popular: dolphin3, llama3.2:1b, llama3.2:3b, mistral, gemma2:7b"; \
		exit 1; \
	fi
	@echo "Pulling Ollama model: $(MODEL)"
	@docker compose exec ollama ollama pull $(MODEL) || \
		docker compose run --rm ollama ollama pull $(MODEL)

llm-import-hf: ## Import GGUF model from Hugging Face into Ollama (usage: make llm-import-hf REPO=user/repo [QUANT=Q4_K_M])
	@if [ -z "$(REPO)" ]; then \
		echo "Usage: make llm-import-hf REPO=<user/repo> [QUANT=<quantization>]"; \
		echo "Example: make llm-import-hf REPO=bartowski/Llama-3.2-1B-Instruct-GGUF"; \
		echo "Example: make llm-import-hf REPO=bartowski/Llama-3.2-3B-Instruct-GGUF QUANT=IQ3_M"; \
		echo "Browse GGUF models: https://huggingface.co/models?library=gguf"; \
		echo "Ollama uses hf.co/REPO; default quant is Q4_K_M when present."; \
		echo ""; \
		echo "Note: Only GGUF repos work. Transformers/Safetensors models (e.g. dphn/dolphin-vision-72b)"; \
		echo "cannot be imported unless a GGUF version exists on Hugging Face."; \
		exit 1; \
	fi
	@HF_MODEL="hf.co/$(REPO)"; \
	if [ -n "$(QUANT)" ]; then HF_MODEL="$$HF_MODEL:$(QUANT)"; fi; \
	echo "Importing from Hugging Face: $$HF_MODEL"; \
	docker compose exec ollama ollama pull "$$HF_MODEL" || \
		docker compose run --rm ollama ollama pull "$$HF_MODEL"

llm-list: ## List installed Ollama models and show available models link
	@echo "=== Available models ==="
	@echo "Browse: https://ollama.com/library"
	@echo ""
	@echo "Default for MagicPrompt (control + compatibility): dolphin3 — https://ollama.com/library/dolphin3"
	@echo "Other options: Light (1-3B) llama3.2:1b, phi3:mini | Balanced (7-8B) mistral, gemma2:7b"
	@echo ""
	@echo "=== Installed models ==="
	@docker compose exec ollama ollama list 2>/dev/null || \
		docker compose run --rm ollama ollama list || \
		(echo "Ollama not running. Start with: make llm" && exit 1)
	@echo ""
	@echo "Pull a model: make llm-pull MODEL=<name>"
	@echo "Import from Hugging Face (GGUF): make llm-import-hf REPO=user/repo [QUANT=...]"
	@echo "Remove a model: make llm-rm MODEL=<name>"

llm-rm: ## Remove an Ollama model (usage: make llm-rm MODEL=dolphin3)
	@if [ -z "$(MODEL)" ]; then \
		echo "Usage: make llm-rm MODEL=<model_name>"; \
		echo "Example: make llm-rm MODEL=dolphin3"; \
		echo "List installed models: make llm-list"; \
		exit 1; \
	fi
	@echo "Removing Ollama model: $(MODEL)"
	@docker compose exec ollama ollama rm $(MODEL) || \
		docker compose run --rm ollama ollama rm $(MODEL)

check-gpu: ## Run GPU diagnostic check
	chmod +x scripts/linux/check-gpu.sh && ./scripts/linux/check-gpu.sh

check-gpu-comfyui: ## Check GPU availability in ComfyUI container
	docker compose exec comfyui python3 -c "import torch; print(f'CUDA available: {torch.cuda.is_available()}'); print(f'GPU count: {torch.cuda.device_count()}'); [torch.cuda.is_available()] and print(f'GPU name: {torch.cuda.get_device_name(0)}')"

check-gpu-swarmui: ## Check GPU availability in SwarmUI container
	docker compose exec swarmui nvidia-smi || echo "nvidia-smi not available, checking CUDA..."
	@docker compose exec swarmui dotnet --version || echo "DotNET check failed"

status: ## Show service status and URLs
	@echo "=== Service Status ==="
	@docker compose ps
	@echo "\n=== Access URLs ==="
	@echo "ComfyUI:         http://localhost:8188"
	@echo "SwarmUI:         http://localhost:7801"
	@echo "Ollama API:      http://localhost:11434"
