# GitLab CI/CD Configuration

This project uses GitLab CI/CD to build Docker images and push them to Harbor registry.

## Required Variables

The following variables must be set at the group level (`dk-raas/dkai/tools`) or project level:

- `HARBOR_REGISTRY` - Harbor registry URL (e.g., `harbor.dataknife.net`)
- `HARBOR_PROJECT` - Harbor project name (e.g., `library`)

These variables are typically set at the group level and inherited by all projects.

## Pipeline Stages

### 1. Lint
- Validates Docker Compose configuration
- Checks YAML syntax
- Validates Makefile
- Checks shell script syntax

### 2. Test
- Tests script syntax
- Validates Dockerfile syntax

### 3. Docker
- Builds and pushes ComfyUI image to Harbor
- Builds and pushes SwarmUI image to Harbor
- Tags: `latest`, commit SHA, and version tag (if tagged)

### 4. Release
- Automatically creates releases on main branch commits
- Version format: `v0.{pipeline_id}`

## Image Tags

Images are pushed with multiple tags:
- `latest` - Always points to the latest build
- `{commit_sha}` - Specific commit SHA
- `{tag}` - Version tag if pipeline is triggered by a git tag

## Harbor Registry

Images are pushed to:
- `$HARBOR_REGISTRY/$HARBOR_PROJECT/freya-comfyui:{tag}`
- `$HARBOR_REGISTRY/$HARBOR_PROJECT/freya-swarmui:{tag}`

## Rules

The pipeline runs:
- On pushes to `main` branch
- On git tags
- On merge requests (lint and test stages only)

## Kubernetes Runners

All jobs use the `kubernetes` tag, requiring GitLab runners with this tag configured.
