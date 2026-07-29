import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  chmodSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";

const npmDirectory = fileURLToPath(new URL(".", import.meta.url));
const repositoryDirectory = fileURLToPath(new URL("..", import.meta.url));
const testRoot = mkdtempSync(`${npmDirectory}/.launcher-test-`);
const testCliDirectory = `${testRoot}/cli`;
const launcher = `${testCliDirectory}/bin/benchguard.js`;
const sentinelDirectory =
  `${testCliDirectory}/node_modules/pre-existing-dependency`;
const sentinelFile = `${sentinelDirectory}/sentinel.txt`;
mkdirSync(`${testCliDirectory}/bin`, { recursive: true });
mkdirSync(sentinelDirectory, { recursive: true });
writeFileSync(sentinelFile, "preserve me");
copyFileSync(
  `${npmDirectory}/cli/package.json`,
  `${testCliDirectory}/package.json`,
);
copyFileSync(`${npmDirectory}/cli/bin/benchguard.js`, launcher);

const cleanup = () => {
  rmSync(testRoot, { recursive: true, force: true });
};
process.once("exit", cleanup);

const target = `${process.platform}-${process.arch}`;
const packageName = target === "win32-x64"
  ? "win32-x64"
  : target === "linux-x64"
    ? "linux-x64"
    : null;

assert.ok(packageName, `launcher smoke test does not support ${target}`);

const cliManifest = JSON.parse(
  readFileSync(`${npmDirectory}/cli/package.json`, "utf8"),
);
assert.equal(cliManifest.engines.node, ">=18");
assert.deepEqual(cliManifest.optionalDependencies, {
  "@benchguard/linux-x64": "0.1.1",
  "@benchguard/win32-x64": "0.1.1",
});

for (const [directory, os] of [
  ["linux-x64", "linux"],
  ["win32-x64", "win32"],
]) {
  const manifest = JSON.parse(
    readFileSync(`${npmDirectory}/${directory}/package.json`, "utf8"),
  );
  assert.deepEqual(manifest.os, [os]);
  assert.deepEqual(manifest.cpu, ["x64"]);
}

const executableName = process.platform === "win32"
  ? "benchguard.exe"
  : "benchguard";
const builtExecutable = `${repositoryDirectory}/target/debug/${executableName}`;
const installedPackage =
  `${testCliDirectory}/node_modules/@benchguard/${packageName}`;

mkdirSync(installedPackage, { recursive: true });
copyFileSync(builtExecutable, `${installedPackage}/${executableName}`);

try {
  const result = spawnSync(
    process.execPath,
    [launcher, "--version"],
    {
      cwd: testRoot,
      encoding: "utf8",
    },
  );

  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /^benchguard 0\.1\.1/);
} finally {
  rmSync(installedPackage, { recursive: true, force: true });
}

const delegationFixture = `${testRoot}/delegation-fixture.cjs`;
writeFileSync(
  delegationFixture,
  [
    'const { readFileSync } = require("node:fs");',
    "const [exitCode, ...args] = process.argv.slice(2);",
    "const stdin = readFileSync(0, \"utf8\");",
    "process.stdout.write(JSON.stringify({ args, stdin }));",
    'process.stderr.write("delegated stderr");',
    "process.exit(Number(exitCode));",
  ].join("\n"),
);
mkdirSync(installedPackage, { recursive: true });
copyFileSync(process.execPath, `${installedPackage}/${executableName}`);

try {
  const delegated = spawnSync(
    process.execPath,
    [
      launcher,
      delegationFixture,
      "37",
      "two words",
      "",
      'embedded "quote"',
      "trailing backslashes \\\\",
    ],
    {
      cwd: testRoot,
      encoding: "utf8",
      input: "delegated stdin",
    },
  );

  assert.equal(delegated.status, 37);
  assert.equal(
    delegated.stdout,
    JSON.stringify({
      args: [
        "two words",
        "",
        'embedded "quote"',
        "trailing backslashes \\\\",
      ],
      stdin: "delegated stdin",
    }),
  );
  assert.equal(delegated.stderr, "delegated stderr");
} finally {
  rmSync(installedPackage, { recursive: true, force: true });
  rmSync(delegationFixture, { force: true });
}

if (process.platform === "linux") {
  const signalFixture = `${testRoot}/signal-fixture.cjs`;
  writeFileSync(signalFixture, 'process.kill(process.pid, "SIGTERM");\n');
  mkdirSync(installedPackage, { recursive: true });
  copyFileSync(process.execPath, `${installedPackage}/${executableName}`);
  chmodSync(`${installedPackage}/${executableName}`, 0o755);

  try {
    const signaled = spawnSync(
      process.execPath,
      [launcher, signalFixture],
      {
        cwd: testRoot,
        encoding: "utf8",
      },
    );

    assert.equal(signaled.status, null);
    assert.equal(signaled.signal, "SIGTERM");
  } finally {
    rmSync(installedPackage, { recursive: true, force: true });
    rmSync(signalFixture, { force: true });
  }
}

const missingPackage = spawnSync(
  process.execPath,
  [launcher, "--version"],
  {
    cwd: testRoot,
    encoding: "utf8",
  },
);

assert.equal(missingPackage.status, 2);
assert.match(
  missingPackage.stderr,
  new RegExp(`Could not find @benchguard/${packageName}`),
);
assert.match(missingPackage.stderr, /Reinstall @benchguard\/cli/);

const unsupportedPreload = `${testRoot}/unsupported-platform.mjs`;
writeFileSync(
  unsupportedPreload,
  [
    'Object.defineProperty(process, "platform", { value: "darwin" });',
    'Object.defineProperty(process, "arch", { value: "arm64" });',
  ].join("\n"),
);

try {
  const unsupported = spawnSync(
    process.execPath,
    [
      "--import",
      pathToFileURL(unsupportedPreload).href,
      launcher,
      "--version",
    ],
    {
      cwd: testRoot,
      encoding: "utf8",
    },
  );

  assert.equal(unsupported.status, 2);
  assert.match(unsupported.stderr, /Unsupported platform darwin-arm64/);
  assert.match(unsupported.stderr, /linux-x64, win32-x64/);
  assert.match(unsupported.stderr, /GitHub release/);
} finally {
  rmSync(unsupportedPreload, { force: true });
}

mkdirSync(installedPackage, { recursive: true });
const failedExecutable = `${installedPackage}/${executableName}`;
if (process.platform === "win32") {
  writeFileSync(failedExecutable, "");
} else {
  const missingInterpreter = `${testRoot}/missing-native-interpreter`;
  writeFileSync(failedExecutable, `#!${missingInterpreter}\n`);
  chmodSync(failedExecutable, 0o755);
}

try {
  const failedSpawn = spawnSync(
    process.execPath,
    [launcher, "--version"],
    {
      cwd: testRoot,
      encoding: "utf8",
    },
  );

  assert.equal(failedSpawn.status, 2);
  assert.match(failedSpawn.stderr, /Could not start the native binary/);
} finally {
  rmSync(installedPackage, { recursive: true, force: true });
}

try {
  assert.equal(readFileSync(sentinelFile, "utf8"), "preserve me");
} finally {
  cleanup();
  process.removeListener("exit", cleanup);
}
