# GitLab Mirror Setup

This repository is configured to automatically mirror to GitLab on pushes to main.

## GitLab Project

- **URL**: https://gitlab.com/dk-raas/dkai/tools/freya
- **Group**: dk-raas/dkai/tools

## Setup GitHub Secret for Auto-Push

To enable automatic pushing to GitLab from GitHub Actions:

1. Go to https://github.com/DataKnifeAI/freya/settings/secrets/actions
2. Click "New repository secret"
3. Name: `GITLAB_TOKEN`
4. Value: Your GitLab personal access token with `api` and `write_repository` scopes
5. Click "Add secret"

### Creating a GitLab Personal Access Token

1. Go to https://gitlab.com/-/user_settings/personal_access_tokens
2. Create a token with:
   - Name: `github-actions-freya`
   - Scopes: **`api`**, **`write_repository`** (both required)
   - Expiration: Set appropriate expiration date
3. Copy the token immediately (it won't be shown again)
4. Add it to GitHub secrets as `GITLAB_TOKEN`

**Important**: The token must have:
- `api` scope - for API access
- `write_repository` scope - for pushing code
- Access to the `dk-raas/dkai/tools` group or the `freya` project

## Manual Push to GitLab

If you need to manually push to GitLab:

```bash
git push gitlab main
```

## GitLab CI/CD

The GitLab CI pipeline runs automatically on:
- Pushes to `main` branch
- Merge requests
- Tags (build stage is manual)

View pipelines at: https://gitlab.com/dk-raas/dkai/tools/freya/-/pipelines
