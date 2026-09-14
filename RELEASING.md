# Releasing what-time

The release is tag-driven: pushing a tag `v*` triggers
[`.github/workflows/release.yml`](.github/workflows/release.yml), which runs
the full gate and publishes `@magicbeansai/what-time` to npm.

## One-time setup (already done / verify once)

| Where | What | Status |
| -- | -- | -- |
| GitHub → Settings → Secrets → Actions | `NPM_TOKEN` secret (npm publish token) | done |
| GitHub → Settings → Environments | `npm` environment (optional protection rules) | done |
| [npmjs.com/org/create](https://www.npmjs.com/org/create) | org named exactly **`magicbeansai`** (free, public plan) — scoped packages cannot publish to a scope that does not exist | **required — publish fails with 404 until this exists** |
| npm token settings | Granular tokens must grant read-and-write on `@magicbeansai`; classic publish tokens work once the org exists | verify |

If the publish step fails and the log shows `ResourceNotFound` or 404, the
org is missing — create it, then re-run the failed jobs from the Actions
page (no re-tag needed).

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

3. Commit, tag, push:

   ```sh
   git add -A && git commit -m "v0.1.1"
   git push origin main
   git tag v0.1.1 && git push origin v0.1.1
   ```

4. The workflow: verifies tag = cargo = npm versions → runs the full Rust
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
