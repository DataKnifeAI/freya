.PHONY: help build build-comfyui build-swarmui up down restart logs clean

setup: ## Run initial directory setup
	chmod +x scripts/setup.sh && ./scripts/setup.sh

quick-setup: ## Interactive setup with model downloads
	chmod +x scripts/quick-setup.sh && ./scripts/quick-setup.sh

download-model: ## Download a model (usage: make download-model TYPE=checkpoint URL=... FILE=...)
	@if [ -z "$(TYPE)" ] || [ -z "$(URL)" ]; then \
		echo "Usage: make download-model TYPE=<type> URL=<url> [FILE=<filename>]"; \
		echo "Types: checkpoint, lora, vae, controlnet, upscale, embedding"; \
		exit 1; \
	fi
	chmod +x scripts/download-model.sh
	./scripts/download-model.sh $(TYPE) $(URL) $(FILE)
	@echo 'Usage: make [target]'
	@echo ''
	@echo 'Available targets:'
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / {printf "  %-15s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

build: ## Build all Docker images
	docker compose build

build-comfyui: ## Build ComfyUI image only
	docker compose build comfyui

build-swarmui: ## Build SwarmUI image only
	docker compose build swarmui

build-no-cache: ## Build all images without cache
	docker compose build --no-cache

up: ## Start all services (ComfyUI and SwarmUI)
	docker compose up -d

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

clean: ## Remove containers, volumes, and images
	docker compose down -v

ps: ## Show running containers
	docker compose ps

shell-comfyui: ## Open shell in ComfyUI container
	docker compose exec comfyui /bin/bash

shell-swarmui: ## Open shell in SwarmUI container
	docker compose exec swarmui /bin/bash

check-gpu-comfyui: ## Check GPU availability in ComfyUI container
	docker compose exec comfyui python3 -c "import torch; print(f'CUDA available: {torch.cuda.is_available()}'); print(f'GPU count: {torch.cuda.device_count()}'); [torch.cuda.is_available()] and print(f'GPU name: {torch.cuda.get_device_name(0)}')"

check-gpu-swarmui: ## Check GPU availability in SwarmUI container
	docker compose exec swarmui nvidia-smi || echo "nvidia-smi not available, checking CUDA..."
	@docker compose exec swarmui dotnet --version || echo "DotNET check failed"

status: ## Show service status and URLs
	@echo "=== Service Status ==="
	@docker compose ps
	@echo "\n=== Access URLs ==="
	@echo "ComfyUI:  http://localhost:8188"
	@echo "SwarmUI:  http://localhost:7801"
