# Release Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the existing immutable `v0.1.0` tag safely releasable after the GitHub Release job failed outside a Git checkout.

**Architecture:** Extend the existing trusted-publisher `release.yml` with a manually dispatched recovery input that selects an existing tag. Pass that tag through checkout, verification, CI, build, GitHub Release, npm, and crates.io jobs, and give `gh release` explicit repository context.

**Tech Stack:** GitHub Actions YAML, Node.js contract tests, GitHub CLI, Rust/Cargo, npm trusted publishing.

## Global Constraints

- Do not move or replace the public `v0.1.0` tag.
- Preserve the release order: GitHub Release, native npm packages, npm CLI, crates.io.
- Keep npm publication tokenless through GitHub OIDC and `npm-production`.
- Keep publication idempotent when a package or release already exists.
- Continue supporting automatic `v*` tag-triggered releases.

---

### Task 1: Capture the failed release contract

**Files:**
- Modify: `.github/scripts/test-release-contract.mjs`
- Test: `.github/scripts/test-release-contract.mjs`

**Interfaces:**
- Consumes: `.github/workflows/release.yml` as UTF-8 text.
- Produces: contract failures when recovery dispatch, selected-tag propagation, or explicit `gh --repo` context is absent.

- [ ] **Step 1: Write the failing test**

Add assertions requiring:

```js
assert.match(release, /workflow_dispatch:[\s\S]*?release_tag:/);
assert.match(
  release,
  /BENCHGUARD_RELEASE_TAG:\s*\$\{\{\s*inputs\.release_tag\s*\|\|\s*github\.ref_name\s*\}\}/,
);
assert.match(
  release,
  /gh release (?:view|create|upload)[^\n]*--repo "\$GITHUB_REPOSITORY"/,
);
```

- [ ] **Step 2: Run test to verify it fails**

Run: `node .github/scripts/test-release-contract.mjs`

Expected: failure because `release.yml` lacks `workflow_dispatch`, `BENCHGUARD_RELEASE_TAG`, and explicit repository context.

- [ ] **Step 3: Commit the red test**

```console
git add .github/scripts/test-release-contract.mjs docs/superpowers/plans/2026-07-29-release-recovery.md
git commit -m "test: capture release recovery contract"
```

### Task 2: Implement tag-safe recovery

**Files:**
- Modify: `.github/workflows/release.yml`
- Modify: `.github/workflows/ci.yml`
- Test: `.github/scripts/test-release-contract.mjs`

**Interfaces:**
- Consumes: optional `workflow_dispatch.inputs.release_tag` such as `v0.1.0`.
- Produces: `BENCHGUARD_RELEASE_TAG` used by every release stage and `source_ref` passed to reusable CI.

- [ ] **Step 1: Add dispatch input and selected-tag environment**

Add required `release_tag` input under `workflow_dispatch`, while preserving the `v*` tag trigger. Define:

```yaml
BENCHGUARD_RELEASE_TAG: ${{ inputs.release_tag || github.ref_name }}
```

- [ ] **Step 2: Propagate the selected tag**

Use `BENCHGUARD_RELEASE_TAG` for version parsing and release commands. Checkout that ref in source-dependent jobs and pass it as `source_ref` to reusable CI.

- [ ] **Step 3: Give GitHub CLI explicit repository context**

Add `--repo "$GITHUB_REPOSITORY"` to `gh release view`, `upload`, and `create`.

- [ ] **Step 4: Run focused tests**

Run:

```console
node .github/scripts/test-release-contract.mjs
node .github/scripts/test-doc-contract.mjs
node .github/scripts/verify-release-version.mjs v0.1.0
```

Expected: all pass and version verification prints `release versions agree on 0.1.0`.

- [ ] **Step 5: Run repository verification**

Run:

```console
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Expected: all commands exit `0`.

- [ ] **Step 6: Commit implementation**

```console
git add .github/workflows/release.yml .github/workflows/ci.yml
git commit -m "fix: support safe release recovery"
```

### Task 3: Publish and resume v0.1.0

**Files:**
- No source files modified.

**Interfaces:**
- Consumes: merged recovery workflow on `master` and immutable tag `v0.1.0`.
- Produces: GitHub Release assets, npm `0.1.0` packages, and crates.io `benchguard 0.1.0`.

- [ ] **Step 1: Push branch and open a pull request**

Push `agent/recover-v0.1.0-release`, open a draft PR, and wait for all required checks.

- [ ] **Step 2: Merge after review**

Mark the PR ready and merge only when all required checks pass.

- [ ] **Step 3: Dispatch recovery**

Run `release.yml` from `master` with `release_tag=v0.1.0`.

- [ ] **Step 4: Approve protected deployments**

Approve `github-release`, `npm-production`, and `crates-io` as each dependency gate opens.

- [ ] **Step 5: Verify public artifacts**

Confirm the GitHub Release and checksums exist, all three npm packages expose `0.1.0`, crates.io exposes `benchguard 0.1.0`, and installation smoke tests report `benchguard 0.1.0`.
