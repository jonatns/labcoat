# Releasing Isomer

Isomer has two separate release workflows.

## Overview

| Release Type | Tag Pattern       | What Gets Built                                       |
| ------------ | ----------------- | ----------------------------------------------------- |
| **Binaries** | `binaries-v0.1.x` | Service dependencies (metashrew, flextrs, espo, etc.) |
| **App**      | `v0.1.x`          | Isomer desktop app (.dmg, .AppImage, .msi)            |

---

## Binaries Release

Builds upstream dependencies that Isomer downloads at runtime.

### When to release:

- Upstream project updates (metashrew, flextrs, espo, alkanes)
- Version bumps in `release-binaries.yml` env variables

### Steps:

1. **Update versions** in `.github/workflows/release-binaries.yml`:

   ```yaml
   env:
     METASHREW_VERSION: "v9.0.3"
     FLEXTRS_VERSION: "0.4.2"
     ESPO_VERSION: "explorer-v9.0.2"
   ```

2. **Tag and push:**

   ```bash
   git add .
   git commit -m "Bump binary versions"
   git tag binaries-v0.1.4
   git push origin main binaries-v0.1.4
   ```

3. **Update Isomer** to point to new binaries:

   ```rust
   // src-tauri/src/binary_manager.rs
   const CHECKSUMS_URL: &str = "https://github.com/jonatns/isomer/releases/download/binaries-v0.1.4/checksums.json";

   let isomer_release_base = "https://github.com/jonatns/isomer/releases/download/binaries-v0.1.4";
   ```

4. Commit the binary_manager.rs update.

---

## App Release

Builds the Isomer desktop application.

### When to release:

- New features or bug fixes
- UI/UX improvements
- After updating binary versions

### Steps:

1. **Bump version** in both files:

   - `package.json`: `"version": "0.1.1"`
   - `src-tauri/Cargo.toml`: `version = "0.1.1"`

2. **Commit and tag:**

   ```bash
   git add .
   git commit -m "Release v0.1.1"
   git tag v0.1.1
   git push origin main v0.1.1
   ```

3. **Publish release:**
   - Go to [GitHub Releases](https://github.com/jonatns/isomer/releases)
   - Find the draft release for `v0.1.1`
   - Click **Generate release notes** to auto-populate from commits
   - Edit notes to be user-friendly (see format below)
   - Click **Publish release**

---

## Release Notes Format

Use this template when editing release notes:

```markdown
## What's New

- ✨ Feature: Description of new feature
- 🐛 Fix: Description of bug fix
- 🔧 Improvement: Description of improvement

## Binary Updates

- Metashrew: v9.0.2 → v9.0.3
- Flextrs: 0.4.1 → 0.4.2

## Installation

curl -sSf https://raw.githubusercontent.com/jonatns/isomer/main/install.sh | bash
```

**Tips:**

- Start each item with an emoji for quick scanning
- Focus on user-facing changes, not internal refactors
- Link to issues/PRs if relevant: `Fixes #123`

---

## Quick Reference

```bash
# Binaries release
git tag binaries-v0.1.4
git push origin binaries-v0.1.4

# App release
git tag v0.1.1
git push origin v0.1.1
```

---

## Troubleshooting

### Build fails

- Check GitHub Actions logs for the specific error
- Common issues: missing dependencies, version mismatches

### Checksum mismatch on download

- A new binaries release was created but `binary_manager.rs` wasn't updated
- Run `git log --oneline binaries-*` to find latest binaries tag
- Update `CHECKSUMS_URL` and `isomer_release_base` to match

### App not finding binaries

- Ensure the binaries release is published (not draft)
- Verify the release tag in `binary_manager.rs` matches an actual release
