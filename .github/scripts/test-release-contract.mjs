import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
} from "node:fs";
import { fileURLToPath } from "node:url";

const repository = fileURLToPath(new URL("../..", import.meta.url));
const read = (path) => readFileSync(`${repository}/${path}`, "utf8");
const packageDirectories = ["cli", "linux-x64", "win32-x64"];
const posixModes = new Map([
  ["cli", 0o640],
  ["linux-x64", 0o600],
  ["win32-x64", 0o644],
]);

async function waitForPath(path, timeoutMilliseconds) {
  const deadline = Date.now() + timeoutMilliseconds;
  while (Date.now() < deadline) {
    if (existsSync(path)) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
  assert.fail(`transaction lock did not appear: ${path}`);
}

const verified = spawnSync(
  process.execPath,
  [".github/scripts/verify-release-version.mjs", "v0.1.0"],
  { cwd: repository, encoding: "utf8" },
);
assert.equal(verified.status, 0, verified.stderr);

const rejected = spawnSync(
  process.execPath,
  [".github/scripts/verify-release-version.mjs", "v0.1.1"],
  { cwd: repository, encoding: "utf8" },
);
assert.notEqual(rejected.status, 0, "a mismatched release tag must fail");

const bootstrapFixture = mkdtempSync(
  `${repository}/.github/scripts/.bootstrap-test-`,
);
try {
  for (const directory of packageDirectories) {
    mkdirSync(`${bootstrapFixture}/npm/${directory}`, { recursive: true });
    copyFileSync(
      `${repository}/npm/${directory}/package.json`,
      `${bootstrapFixture}/npm/${directory}/package.json`,
    );
    if (process.platform !== "win32") {
      chmodSync(
        `${bootstrapFixture}/npm/${directory}/package.json`,
        posixModes.get(directory),
      );
    }
  }
  const prepared = spawnSync(
    process.execPath,
    [".github/scripts/prepare-npm-manifests.mjs", "0.0.0-bootstrap.0"],
    {
      cwd: repository,
      encoding: "utf8",
      env: {
        ...process.env,
        BENCHGUARD_REPOSITORY: bootstrapFixture,
        BENCHGUARD_GITHUB_SERVER_URL: "https://github.com",
        BENCHGUARD_GITHUB_REPOSITORY: "benchguard-project/benchguard",
      },
    },
  );
  assert.equal(prepared.status, 0, prepared.stderr);
  const preparedCli = JSON.parse(
    readFileSync(`${bootstrapFixture}/npm/cli/package.json`, "utf8"),
  );
  assert.equal(preparedCli.version, "0.0.0-bootstrap.0");
  assert.deepEqual(preparedCli.optionalDependencies, {
    "@benchguard/linux-x64": "0.0.0-bootstrap.0",
    "@benchguard/win32-x64": "0.0.0-bootstrap.0",
  });
  for (const directory of ["linux-x64", "win32-x64"]) {
    const manifest = JSON.parse(
      readFileSync(`${bootstrapFixture}/npm/${directory}/package.json`, "utf8"),
    );
    assert.equal(manifest.version, "0.0.0-bootstrap.0");
  }
  for (const directory of packageDirectories) {
    const manifest = JSON.parse(
      readFileSync(`${bootstrapFixture}/npm/${directory}/package.json`, "utf8"),
    );
    assert.deepEqual(manifest.repository, {
      type: "git",
      url: "git+https://github.com/benchguard-project/benchguard.git",
      directory: `npm/${directory}`,
    });
    if (process.platform !== "win32") {
      assert.equal(
        statSync(`${bootstrapFixture}/npm/${directory}/package.json`).mode &
          0o777,
        posixModes.get(directory),
        `${directory} mode must survive successful replacement`,
      );
    }
  }

  const rejectedMetadata = spawnSync(
    process.execPath,
    [".github/scripts/prepare-npm-manifests.mjs"],
    {
      cwd: repository,
      encoding: "utf8",
      env: {
        ...process.env,
        BENCHGUARD_REPOSITORY: bootstrapFixture,
        BENCHGUARD_GITHUB_SERVER_URL: "https://github.com",
        BENCHGUARD_GITHUB_REPOSITORY: "owner/repo;touch injected",
      },
    },
  );
  assert.notEqual(
    rejectedMetadata.status,
    0,
    "unsafe GitHub repository metadata must be rejected",
  );
  assert.equal(existsSync(`${bootstrapFixture}/injected`), false);

  const failureFixture = `${bootstrapFixture}/transaction-failure`;
  const originalManifests = new Map();
  for (const directory of packageDirectories) {
    mkdirSync(`${failureFixture}/npm/${directory}`, { recursive: true });
    const source = `${repository}/npm/${directory}/package.json`;
    const destination = `${failureFixture}/npm/${directory}/package.json`;
    copyFileSync(source, destination);
    if (process.platform !== "win32") {
      chmodSync(destination, posixModes.get(directory));
    }
    originalManifests.set(directory, readFileSync(destination));
  }
  const failedTransaction = spawnSync(
    process.execPath,
    [".github/scripts/prepare-npm-manifests.mjs", "0.0.0-bootstrap.0"],
    {
      cwd: repository,
      encoding: "utf8",
      env: {
        ...process.env,
        BENCHGUARD_REPOSITORY: failureFixture,
        BENCHGUARD_GITHUB_SERVER_URL: "https://github.com",
        BENCHGUARD_GITHUB_REPOSITORY: "benchguard-project/benchguard",
        BENCHGUARD_TEST_FAIL_AFTER_REPLACES: "1",
      },
    },
  );
  assert.notEqual(
    failedTransaction.status,
    0,
    "an injected mid-finalization failure must be reported",
  );
  for (const directory of packageDirectories) {
    const packageDirectory = `${failureFixture}/npm/${directory}`;
    assert.deepEqual(
      readFileSync(`${packageDirectory}/package.json`),
      originalManifests.get(directory),
      `${directory} must be restored byte-for-byte after failure`,
    );
    assert.deepEqual(
      readdirSync(packageDirectory),
      ["package.json"],
      `${directory} must not retain transaction debris`,
    );
    if (process.platform !== "win32") {
      assert.equal(
        statSync(`${packageDirectory}/package.json`).mode & 0o777,
        posixModes.get(directory),
        `${directory} mode must survive rollback`,
      );
    }
  }

  const concurrencyFixture = `${bootstrapFixture}/concurrency`;
  for (const directory of packageDirectories) {
    mkdirSync(`${concurrencyFixture}/npm/${directory}`, { recursive: true });
    copyFileSync(
      `${repository}/npm/${directory}/package.json`,
      `${concurrencyFixture}/npm/${directory}/package.json`,
    );
  }
  const commonEnvironment = {
    ...process.env,
    BENCHGUARD_REPOSITORY: concurrencyFixture,
    BENCHGUARD_GITHUB_SERVER_URL: "https://github.com",
  };
  const faultyProcess = spawn(
    process.execPath,
    [".github/scripts/prepare-npm-manifests.mjs", "0.0.0-bootstrap.0"],
    {
      cwd: repository,
      env: {
        ...commonEnvironment,
        BENCHGUARD_GITHUB_REPOSITORY: "first/faulty",
        BENCHGUARD_TEST_FAIL_AFTER_REPLACES: "1",
        BENCHGUARD_TEST_HOLD_AFTER_REPLACES_MS: "1000",
      },
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  let faultyStderr = "";
  faultyProcess.stderr.on("data", (chunk) => {
    faultyStderr += chunk;
  });
  const faultyClosed = new Promise((resolve) => {
    faultyProcess.on("close", (status) => resolve(status));
  });
  const lockPath = `${concurrencyFixture}/.benchguard-npm-manifests.lock`;
  await waitForPath(lockPath, 1000);

  const contender = spawnSync(
    process.execPath,
    [".github/scripts/prepare-npm-manifests.mjs", "0.0.0-bootstrap.1"],
    {
      cwd: repository,
      encoding: "utf8",
      env: {
        ...commonEnvironment,
        BENCHGUARD_GITHUB_REPOSITORY: "second/contender",
      },
    },
  );
  assert.notEqual(contender.status, 0, "a concurrent transaction must fail");
  assert.match(
    contender.stderr,
    /another npm manifest transaction holds .*confirming no process owns it/i,
    "lock contention must explain safe stale-lock recovery",
  );
  assert.notEqual(await faultyClosed, 0, faultyStderr);
  assert.equal(existsSync(lockPath), false, "the owner must release its lock");

  const winner = spawnSync(
    process.execPath,
    [".github/scripts/prepare-npm-manifests.mjs", "0.0.0-bootstrap.1"],
    {
      cwd: repository,
      encoding: "utf8",
      env: {
        ...commonEnvironment,
        BENCHGUARD_GITHUB_REPOSITORY: "second/winner",
      },
    },
  );
  assert.equal(winner.status, 0, winner.stderr);
  for (const directory of packageDirectories) {
    const manifest = JSON.parse(
      readFileSync(`${concurrencyFixture}/npm/${directory}/package.json`),
    );
    assert.equal(manifest.version, "0.0.0-bootstrap.1");
    assert.equal(
      manifest.repository.url,
      "git+https://github.com/second/winner.git",
      "a released lock must allow one complete, non-mixed transaction",
    );
  }
} finally {
  rmSync(bootstrapFixture, { recursive: true, force: true });
}

const ci = read(".github/workflows/ci.yml");
for (const contract of [
  "pull_request:",
  "push:",
  "workflow_call:",
  "ubuntu-latest",
  "windows-latest",
  "cargo fmt --check",
  "cargo clippy --workspace --all-targets -- -D warnings",
  "cargo test --workspace --locked",
  "cargo build --workspace --locked",
  "check --workspace --all-targets --locked",
  "- 18",
  "- lts/*",
  "npm pack",
  "node npm/test-launcher.mjs",
  "cargo test --test failures linux_ -- --nocapture",
  "cygpath -u \"$RUNNER_TEMP\"",
  "cp README.md LICENSE-MIT LICENSE-APACHE npm/cli/",
  "BENCHGUARD_GITHUB_SERVER_URL: ${{ github.server_url }}",
  "BENCHGUARD_GITHUB_REPOSITORY: ${{ github.repository }}",
  "node .github/scripts/prepare-npm-manifests.mjs",
]) {
  assert.match(ci, new RegExp(escapeRegex(contract)), `CI is missing ${contract}`);
}

const release = read(".github/workflows/release.yml");
for (const contract of [
  "tags:",
  "- \"v*\"",
  "verify-release-version.mjs",
  "uses: ./.github/workflows/ci.yml",
  "x86_64-unknown-linux-gnu",
  "x86_64-pc-windows-msvc",
  "README.md",
  "LICENSE-MIT",
  "LICENSE-APACHE",
  "SHA256SUMS",
  "gh release",
  "environment: github-release",
  "environment: npm-production",
  "environment: crates-io",
  "id-token: write",
  "npm publish",
  "cargo publish --locked",
  "cp README.md LICENSE-MIT LICENSE-APACHE npm/cli/",
  "Run npm-bootstrap.yml first",
  "BENCHGUARD_GITHUB_SERVER_URL: ${{ github.server_url }}",
  "BENCHGUARD_GITHUB_REPOSITORY: ${{ github.repository }}",
]) {
  assert.match(
    release,
    new RegExp(escapeRegex(contract)),
    `release workflow is missing ${contract}`,
  );
}

assert.match(
  release,
  /publish-npm-cli:[\s\S]*?\n    needs: publish-npm-native/,
  "the CLI package must wait for native npm packages",
);
assert.match(
  release,
  /publish-cargo:[\s\S]*?\n    needs: publish-npm-cli/,
  "Cargo publication must follow npm publication",
);
assert.doesNotMatch(release, /secrets\.NPM_TOKEN/);
assert.equal(
  release.match(/node \.github\/scripts\/prepare-npm-manifests\.mjs/g)?.length,
  2,
  "release must prepare repository metadata for native and CLI manifests",
);
assert.doesNotMatch(
  ci,
  /\$RUNNER_TEMP\/benchguard-/,
  "Git Bash must not concatenate POSIX paths onto raw Windows RUNNER_TEMP",
);

const bootstrap = read(".github/workflows/npm-bootstrap.yml");
for (const contract of [
  "workflow_dispatch:",
  "Own or create the @benchguard npm user or organization scope.",
  "Limit the bootstrap token to the @benchguard scope with read-write package/scope permission.",
  "Allowed trusted-publisher action: npm publish.",
  "environment: npm-bootstrap",
  "permissions:",
  "contents: read",
  "secrets.NPM_TOKEN",
  "BOOTSTRAP @benchguard PACKAGES",
  "0.0.0-bootstrap.0",
  "--tag bootstrap",
  "README.md",
  "LICENSE-MIT",
  "LICENSE-APACHE",
  "publish-npm-native:",
  "publish-npm-cli:",
  "needs: publish-npm-native",
  "already exists; bootstrap is unnecessary",
  "Configure npm trusted publishing for release.yml",
  "BENCHGUARD_GITHUB_SERVER_URL: ${{ github.server_url }}",
  "BENCHGUARD_GITHUB_REPOSITORY: ${{ github.repository }}",
]) {
  assert.match(
    bootstrap,
    new RegExp(escapeRegex(contract)),
    `npm bootstrap workflow is missing ${contract}`,
  );
}
assert.doesNotMatch(bootstrap, /id-token:\s*write/);
assert.doesNotMatch(bootstrap, /^\s+push:/m);
assert.equal(
  bootstrap.match(/node \.github\/scripts\/prepare-npm-manifests\.mjs/g)?.length,
  2,
  "bootstrap must prepare repository metadata for native and CLI manifests",
);
assert.equal(
  bootstrap.match(/secrets\.NPM_TOKEN/g)?.length,
  2,
  "only the two approval-gated publishing jobs receive NPM_TOKEN",
);
assert.match(
  bootstrap,
  /publish-npm-native:[\s\S]*?environment: npm-bootstrap/,
);
assert.match(
  bootstrap,
  /publish-npm-cli:[\s\S]*?environment: npm-bootstrap/,
);

for (const workflow of [ci, release, bootstrap]) {
  for (const match of workflow.matchAll(/^\s*uses:\s+([^./\s][^@\s]*)@([^\s#]+)/gm)) {
    assert.match(
      match[2],
      /^[0-9a-f]{40}$/,
      `${match[1]} must be pinned to a full commit SHA`,
    );
  }
}

const cargo = read("Cargo.toml");
assert.match(cargo, /\[\[bench\]\][\s\S]*name = "core"[\s\S]*harness = false/);
assert.match(cargo, /\[profile\.release\][\s\S]*strip = "symbols"/);

function escapeRegex(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
