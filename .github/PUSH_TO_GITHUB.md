# Push Freya to GitHub

## Step 1: Create Repository on GitHub

1. Go to https://github.com/DataKnifeAI
2. Click "New repository"
3. Repository name: `freya`
4. Description: "ComfyUI & SwarmUI Stable Diffusion Platform - Kubernetes-ready platform with GPU support"
5. Visibility: Public (or Private as needed)
6. **DO NOT** initialize with README, .gitignore, or license (we already have these)
7. Click "Create repository"

## Step 2: Add Remote and Push

After creating the repository, run these commands:

```bash
# Add the GitHub remote
git remote add origin https://github.com/DataKnifeAI/freya.git

# Verify remote
git remote -v

# Push to GitHub
git push -u origin main
```

## Alternative: Using SSH

If you have SSH keys set up with GitHub:

```bash
git remote add origin git@github.com:DataKnifeAI/freya.git
git push -u origin main
```

## Step 3: Verify

After pushing, verify at:
- https://github.com/DataKnifeAI/freya
