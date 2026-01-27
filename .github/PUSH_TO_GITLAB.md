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

## Troubleshooting Authentication

If you see "Access denied" errors:

1. **Verify token scopes**:
   - Go to https://gitlab.com/-/user_settings/personal_access_tokens
   - Check that your token has both `api` and `write_repository` scopes
   - Ensure the token hasn't expired

2. **Verify project access**:
   - The token must have access to the `dk-raas/dkai/tools` group
   - Or be a project access token for the `freya` project specifically

3. **Check token type**:
   - Personal Access Token: Works for personal projects
   - Project Access Token: Works for specific projects (Premium+)
   - Group Access Token: Works for all projects in a group

4. **Test token manually**:
   ```bash
   # Test API access
   curl --header "PRIVATE-TOKEN: YOUR_TOKEN" https://gitlab.com/api/v4/user
   
   # Test project access
   curl --header "PRIVATE-TOKEN: YOUR_TOKEN" \
     https://gitlab.com/api/v4/projects/dk-raas%2Fdkai%2Ftools%2Ffreya
   ```

5. **If token works in other projects**:
   - Ensure the token has access to the `dk-raas/dkai/tools` group
   - Check if group/project permissions have changed
   - Verify the token hasn't been revoked or expired

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
