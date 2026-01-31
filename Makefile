.PHONY: help check-deps build build-comfyui build-swarmui up down sui sui-rebuild cui llm llm-pull llm-import-hf llm-list llm-rm llm-logs swarmui-rebuild swarmui-extensions-check restart logs clean

help: ## Show this help message
	@echo 'Usage: make [target]'
	@echo ''
	@echo 'Available targets:'
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / {printf "  %-15s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

check-deps: ## Check Docker, Docker Compose, Git; install Git on Linux if missing; warn on NVIDIA
	chmod +x scripts/linux/check-deps.sh && ./scripts/linux/check-deps.sh

setup: ## Run initial directory setup (runs check-deps first, then creates dirs)
	chmod +x scripts/linux/setup.sh && ./scripts/linux/setup.sh

quick-setup: ## Non-interactive setup: directories + ComfyUI starter models (SDXL + VAE)
	chmod +x scripts/linux/quick-setup.sh && ./scripts/linux/quick-setup.sh

download-starter-models: ## Download ComfyUI starter models (SDXL base + VAE, ~7.3 GB)
	chmod +x scripts/linux/download-model.sh && ./scripts/linux/download-model.sh starter

download-model: ## Download one ComfyUI model (make download-model TYPE=checkpoint URL=... [FILE=...])
	@if [ -z "$(TYPE)" ] || [ -z "$(URL)" ]; then \
		echo "Usage: make download-model TYPE=<type> URL=<url> [FILE=<filename>]"; \
		echo "Types: checkpoint, lora, vae, controlnet, upscale, embedding"; \
		echo "See docs/model-downloads.md for Hugging Face / Civitai (account + API key required)."; \
		exit 1; \
	fi
	chmod +x scripts/linux/download-model.sh
	./scripts/linux/download-model.sh "$(TYPE)" "$(URL)" "$(FILE)"

build: ## Build all Docker images
	docker compose build

build-comfyui: ## Build ComfyUI image only
	docker compose build comfyui

build-swarmui: ## Build SwarmUI image only
	docker compose build swarmui

build-no-cache: ## Build all images without cache
	docker compose build --no-cache

up: ## Start all services
	docker compose up -d

sui: ## SwarmUI + Ollama (stops other compose services first)
	docker compose down && docker compose up -d swarmui ollama

sui-rebuild: ## Start SwarmUI + Ollama, then recompile extensions (slower; use after adding/changing extensions)
	docker compose down && docker compose up -d swarmui ollama && \
	sleep 5 && $(MAKE) swarmui-rebuild

cui: ## ComfyUI only (stops other compose services first)
	docker compose down && docker compose up -d comfyui

llm: ## Ollama only (for SwarmUI MagicPrompt / LLM API; stops other compose services first)
	docker compose down && docker compose up -d ollama

down: ## Stop all services
	docker compose down

restart: ## Restart all services
	docker compose restart

logs: ## Show logs from all services
	docker compose logs -f

logs-comfyui: ## Show logs from ComfyUI service
	docker compose logs -f comfyui

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

swarmui-rebuild: ## Recompile SwarmUI (use SwarmUI update script if present, else dotnet build); then restart: make down && make sui
	docker compose exec swarmui bash -c "cd /app/SwarmUI && (test -x ./launchtools/update-linuxmac.sh && ./launchtools/update-linuxmac.sh || (cd src && dotnet build SwarmUI.csproj -c Release))"

swarmui-extensions-check: ## List extension dir and run build (use if extension still not seen after rebuild)
	@echo "=== Extension dir (host: swarmui/extensions) ==="
	@docker compose exec swarmui ls -la /app/SwarmUI/src/Extensions/ 2>/dev/null || true
	@echo "=== Extension subdirs (.cs) ==="
	@docker compose exec swarmui find /app/SwarmUI/src/Extensions -maxdepth 2 -type f -name "*.cs" 2>/dev/null || true
	@echo "=== Rebuild (update script or dotnet build; watch for errors) ==="
	docker compose exec swarmui bash -c "cd /app/SwarmUI && (test -x ./launchtools/update-linuxmac.sh && ./launchtools/update-linuxmac.sh || (cd src && dotnet build SwarmUI.csproj -c Release -v minimal))"

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
