import assert from "node:assert/strict";
import {
  closeSync,
  fchmodSync,
  fsyncSync,
  mkdirSync,
  openSync,
  readFileSync,
  renameSync,
  rmdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";

const packageRoot = process.env.BENCHGUARD_REPOSITORY ??
  fileURLToPath(new URL("../..", import.meta.url));
const githubServerUrl = process.env.BENCHGUARD_GITHUB_SERVER_URL;
const githubRepository = process.env.BENCHGUARD_GITHUB_REPOSITORY;
const failAfterReplaces = process.env.BENCHGUARD_TEST_FAIL_AFTER_REPLACES;
const holdAfterReplaces = process.env.BENCHGUARD_TEST_HOLD_AFTER_REPLACES_MS;
const bootstrapVersion = process.argv[2];

assert.match(
  githubRepository ?? "",
  /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/,
  "BENCHGUARD_GITHUB_REPOSITORY must be an owner/repository pair",
);

const server = new URL(githubServerUrl);
assert.equal(server.protocol, "https:", "GitHub server URL must use HTTPS");
assert.equal(server.username, "", "GitHub server URL must not contain credentials");
assert.equal(server.password, "", "GitHub server URL must not contain credentials");
assert.equal(server.search, "", "GitHub server URL must not contain a query");
assert.equal(server.hash, "", "GitHub server URL must not contain a fragment");

if (bootstrapVersion !== undefined) {
  assert.match(
    bootstrapVersion,
    /^0\.0\.0-bootstrap\.\d+$/,
    "bootstrap version must use the 0.0.0-bootstrap.N form",
  );
}
if (failAfterReplaces !== undefined) {
  assert.match(
    failAfterReplaces,
    /^[1-3]$/,
    "BENCHGUARD_TEST_FAIL_AFTER_REPLACES must be between 1 and 3",
  );
}
if (holdAfterReplaces !== undefined) {
  assert.match(
    holdAfterReplaces,
    /^[1-9]\d{0,3}$/,
    "BENCHGUARD_TEST_HOLD_AFTER_REPLACES_MS must be between 1 and 9999",
  );
  assert.notEqual(
    failAfterReplaces,
    undefined,
    "a test hold requires an injected replacement failure",
  );
}

function prepareManifests() {
const serverPath = server.pathname.replace(/\/+$/, "");
const repositoryUrl =
  `git+${server.protocol}//${server.host}${serverPath}/${githubRepository}.git`;
const manifests = new Map();

for (const directory of ["cli", "linux-x64", "win32-x64"]) {
  const path = `${packageRoot}/npm/${directory}/package.json`;
  const manifest = JSON.parse(readFileSync(path, "utf8"));
  manifest.repository = {
    type: "git",
    url: repositoryUrl,
    directory: `npm/${directory}`,
  };
  if (bootstrapVersion !== undefined) {
    manifest.version = bootstrapVersion;
  }
  manifests.set(directory, { manifest, path });
}

if (bootstrapVersion !== undefined) {
  const cli = manifests.get("cli").manifest;
  cli.optionalDependencies["@benchguard/linux-x64"] = bootstrapVersion;
  cli.optionalDependencies["@benchguard/win32-x64"] = bootstrapVersion;
}

const transactions = [...manifests.values()].map(({ manifest, path }) => {
  const transaction = `${path}.benchguard-${process.pid}-${randomUUID()}`;
  return {
    manifest,
    path,
    original: readFileSync(path),
    mode: statSync(path).mode & 0o777,
    nextPath: `${transaction}.next`,
    backupPath: `${transaction}.backup`,
  };
});

function writeDurably(path, contents, mode) {
  const descriptor = openSync(path, "wx", mode);
  try {
    writeFileSync(descriptor, contents);
    fchmodSync(descriptor, mode);
    fsyncSync(descriptor);
  } finally {
    closeSync(descriptor);
  }
}

function cleanup(path) {
  if (path !== undefined) {
    rmSync(path, { force: true });
  }
}

try {
  for (const transaction of transactions) {
    writeDurably(
      transaction.nextPath,
      `${JSON.stringify(transaction.manifest, null, 2)}\n`,
      transaction.mode,
    );
    writeDurably(
      transaction.backupPath,
      transaction.original,
      transaction.mode,
    );
  }
} catch (error) {
  for (const transaction of transactions) {
    cleanup(transaction.nextPath);
    cleanup(transaction.backupPath);
  }
  throw error;
}

const replaced = [];
try {
  for (const transaction of transactions) {
    renameSync(transaction.nextPath, transaction.path);
    transaction.nextPath = undefined;
    replaced.push(transaction);
    if (replaced.length === Number(failAfterReplaces)) {
      if (holdAfterReplaces !== undefined) {
        Atomics.wait(
          new Int32Array(new SharedArrayBuffer(4)),
          0,
          0,
          Number(holdAfterReplaces),
        );
      }
      throw new Error(
        `injected failure after ${failAfterReplaces} manifest replacement(s)`,
      );
    }
  }
} catch (error) {
  const rollbackErrors = [];
  const retainedBackups = new Set();
  for (const transaction of replaced.reverse()) {
    try {
      renameSync(transaction.backupPath, transaction.path);
      transaction.backupPath = undefined;
    } catch (rollbackError) {
      rollbackErrors.push(rollbackError);
      retainedBackups.add(transaction.backupPath);
    }
  }
  for (const transaction of transactions) {
    cleanup(transaction.nextPath);
    if (!retainedBackups.has(transaction.backupPath)) {
      cleanup(transaction.backupPath);
    }
  }
  if (rollbackErrors.length > 0) {
    throw new AggregateError(
      [error, ...rollbackErrors],
      "manifest transaction failed and rollback was incomplete",
    );
  }
  throw error;
}

for (const transaction of transactions) {
  cleanup(transaction.backupPath);
}
}

function acquireLock() {
  const path = `${packageRoot}/.benchguard-npm-manifests.lock`;
  const ownerPath = `${path}/owner.json`;
  const token = randomUUID();
  try {
    mkdirSync(path, { mode: 0o700 });
  } catch (error) {
    if (error.code === "EEXIST") {
      throw new Error(
        `another npm manifest transaction holds ${path}; after confirming no process owns it, remove that exact lock directory and retry`,
        { cause: error },
      );
    }
    throw error;
  }
  try {
    writeFileSync(
      ownerPath,
      `${JSON.stringify({
        token,
        pid: process.pid,
        acquiredAt: new Date().toISOString(),
      })}\n`,
      { encoding: "utf8", flag: "wx", mode: 0o600 },
    );
  } catch (error) {
    try {
      rmdirSync(path);
    } catch {
      // Preserve a lock directory whose contents are not owned by this process.
    }
    throw error;
  }
  return { ownerPath, path, token };
}

function releaseLock(lock) {
  const owner = JSON.parse(readFileSync(lock.ownerPath, "utf8"));
  if (owner.token !== lock.token) {
    throw new Error(
      `npm manifest lock ownership changed at ${lock.path}; refusing to remove it`,
    );
  }
  rmSync(lock.ownerPath);
  rmdirSync(lock.path);
}

const lock = acquireLock();
let transactionError;
try {
  prepareManifests();
} catch (error) {
  transactionError = error;
}

let releaseError;
try {
  releaseLock(lock);
} catch (error) {
  releaseError = error;
}

if (transactionError !== undefined && releaseError !== undefined) {
  throw new AggregateError(
    [transactionError, releaseError],
    "manifest transaction and lock release both failed",
  );
}
if (transactionError !== undefined) {
  throw transactionError;
}
if (releaseError !== undefined) {
  throw releaseError;
}
