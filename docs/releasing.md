# Release checklist

This is an operator checklist, not evidence that a release has already been
published. Run it from a clean, reviewed commit. Do not publish from an
uncommitted working tree.

## One-time repository administration

1. Confirm the GitHub repository owner/name and reserve `benchguard` on
   crates.io. Before trying to create any npm package, own the `@benchguard`
   user scope or create/join the npm organization named `benchguard` with
   permission to publish in that scope. An organization-scoped package cannot
   be created until that organization exists; package names inside an owned
   scope remain absent until their first publish.
2. Create protected GitHub environments named `npm-bootstrap`,
   `npm-production`, `github-release`, and `crates-io`. Require trusted
   reviewers for every publishing environment.
3. Because the three packages do not exist yet, create a short-lived granular
   npm token for the owned **scope** `@benchguard`, not for nonexistent package
   resources. Give Packages and scopes `Read and write` permission, choose the
   shortest practical expiry, and enable bypass 2FA for this non-interactive
   first publication. The token cannot exceed the npm user's permission in the
   scope. Store it as `NPM_TOKEN` only in the protected `npm-bootstrap`
   environment.
4. Manually run `.github/workflows/npm-bootstrap.yml`, typing the exact
   confirmation `BOOTSTRAP @benchguard PACKAGES`. The workflow refuses partial
   package-name states and publishes native bootstrap packages before the CLI.
5. After all three packages exist, configure a Trusted Publisher separately in
   each package's settings. Select GitHub Actions and enter the GitHub
   organization/user and repository name, workflow filename `release.yml`
   (filename only, not `.github/workflows/release.yml`), environment
   `npm-production`, and allowed action `npm publish`. Current npm
   configurations require at least one allowed action; BenchGuard's workflow
   uses direct `npm publish`, not staged publishing.
6. Delete the GitHub `NPM_TOKEN` secret and revoke the short-lived npm token.
   Then set each package's publishing access to require 2FA and disallow
   tokens. Normal tag releases use OIDC and must not retain or consume the
   bootstrap token.
7. Add a scoped crates.io API token as `CARGO_REGISTRY_TOKEN` in the protected
   `crates-io` environment. GitHub Releases use the repository token.

Environment reviewers and npm trusted-publisher relationships live in GitHub
and npm administration; workflow YAML cannot create them.

The machine-checked administrator profile is:

```json
{
  "npm_bootstrap_admin": {
    "scope": {
      "name": "@benchguard",
      "ownership": "owned npm user or organization scope",
      "packages_must_be_absent": true
    },
    "token": {
      "resource_type": "scope",
      "resource": "@benchguard",
      "permission": "read-write",
      "bypass_2fa": true,
      "lifetime": "short-lived"
    },
    "trusted_publisher": {
      "provider": "GitHub Actions",
      "workflow_filename": "release.yml",
      "environment": "npm-production",
      "allowed_actions": ["npm publish"]
    }
  }
}
```

These steps follow npm's guidance for
[scoped public packages](https://docs.npmjs.com/creating-and-publishing-scoped-public-packages/),
[granular token scope permissions](https://docs.npmjs.com/cli/npm-token/), and
[trusted publishers](https://docs.npmjs.com/trusted-publishers/). Check those
official pages again immediately before first publication because registry
controls can change.

## Per-release preparation

- [ ] The release version matches `Cargo.toml` and all three npm manifests.
- [ ] `CHANGELOG.md` describes Added, Changed, Fixed, and Security impact.
- [ ] `cargo fmt --check` passes.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes.
- [ ] `cargo test --workspace --locked` passes on Windows and Linux.
- [ ] The five-run Linux process-group reliability loop passes.
- [ ] `cargo bench --bench core -- --test` passes.
- [ ] `node npm/test-launcher.mjs` passes on Windows and Linux.
- [ ] `node .github/scripts/test-release-contract.mjs` passes.
- [ ] `node .github/scripts/test-doc-contract.mjs` passes.
- [ ] `node scripts/test-readme-acceptance.mjs` passes.
- [ ] `node scripts/readme-acceptance.mjs` passes.
- [ ] `cargo package --locked` verifies the crate contents and build.
- [ ] `npm pack --dry-run --json` contains the launcher/native file,
      package-local README, and both licenses for all three packages.
- [ ] `benchguard help` and `benchguard help record` document commands,
      defaults, and `-r`, `-w`, `-t`, and `-f`.
- [ ] Normal checks return `0`, an intentional regression returns `1`, and a
      timeout returns `2` after managed-scope cleanup.
- [ ] A failed `record` leaves the existing baseline bytes unchanged.
- [ ] Cargo, npm launcher, tag, and release binary versions are identical.
- [ ] The sample baseline remains schema v1 and matches the JSON reference.

## Publish

1. Merge the reviewed release commit and wait for CI on Windows and Linux.
2. Create and push the exact annotated tag, for example `v0.1.0`.
3. Approve the protected environments only after the release workflow's
   version and CI gates pass.
4. Confirm the workflow builds both x86-64 archives, creates `SHA256SUMS`,
   publishes the GitHub Release, publishes native npm packages before the npm
   CLI, and publishes the Cargo crate last.
5. Confirm `npm install --global @benchguard/cli`, `npx @benchguard/cli`,
   `cargo install benchguard --locked`, and both downloaded binaries report the
   same version and complete the README quick start.
6. Verify checksums from a fresh download and inspect package pages for the
   expected README, dual license, repository metadata, platform constraints,
   and files.

If any external version already exists, do not overwrite it. The workflows
skip an identical published version and stop on an unsafe or partial bootstrap
state.
