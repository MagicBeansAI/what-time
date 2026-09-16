# Releasing what-time

The release is tag-driven: pushing a tag `v*` triggers
[`.github/workflows/release.yml`](.github/workflows/release.yml), which runs
the full gate and publishes `@magicbeansai/what-time` to npm.

## One-time setup (already done / verify once)

| Where | What | Status |
| -- | -- | -- |
| GitHub → Settings → Secrets → Actions | `NPM_TOKEN` secret (npm publish token) | done |
| GitHub → Settings → Environments | `npm` environment (optional protection rules) | done |
| [npmjs.com/org/create](https://www.npmjs.com/org/create) | org named exactly **`magicbeansai`** (free, public plan) — scoped packages cannot publish to a scope that does not exist | done |
| npm token | must be a **classic Automation token** (or granular with "Bypass 2FA" enabled) — plain granular tokens fail publish with `403 Two-factor authentication or granular access token with bypass 2fa enabled is required` | done |

If the publish step fails:
- **404 / ResourceNotFound** — the npm org is missing; create it at
  [npmjs.com/org/create](https://www.npmjs.com/org/create), then re-run the
  failed jobs from the Actions page (no re-tag needed).
- **403 "Two-factor authentication or granular access token with bypass 2fa
  enabled is required"** — the `NPM_TOKEN` secret is a granular token
  without 2FA bypass. Replace it with a **classic Automation token**
  (npmjs.com → Access Tokens → Generate → Classic → Automation) and update
  the secret.

## Release ritual

1. Bump the version in **both** places (they must agree or the workflow
   fails before publishing — that gate is intentional):

   ```sh
   # rust/Cargo.toml → [workspace.package] version = "0.1.1"
   # packages/what-time/package.json → "version": "0.1.1"
   ```

2. Regenerate everything from the new version:

   ```sh
   rust/site/build.sh                                  # wasm + single-file playground (badge version)
   pnpm --filter @magicbeansai/what-time build         # wrapper dist + VERSION export
   ```

3. Append a row to `SCORES.md` for the checkpoint being shipped (never
   edit old rows). The scores come from the training run's final epoch
   report plus the corpus counts `cargo test` prints.

4. Commit, tag, push:

   ```sh
   git add -A && git commit -m "v0.1.1"
   git push origin main
   git tag v0.1.1 && git push origin v0.1.1
   ```

5. The workflow: verifies tag = cargo = npm versions → runs the full Rust
   test gate → builds the wasm fresh from the tag → builds and tests the
   wrapper against it → `npm publish --access public` → attaches
   `what-time.html` and the CLI binary as artifacts.

Watch it: **Actions → Release**. A failed publish can be retried with
"Re-run failed jobs" after fixing the cause (usually the npm org or token).

## Manual publish (escape hatch)

```sh
cd packages/what-time
npm publish --access public    # reads ~/.npmrc for the token
```

The workflow runs `prepublishOnly` (build + tests), so a broken package
cannot ship from either path.
