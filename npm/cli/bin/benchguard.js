#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { writeSync } from "node:fs";
import { createRequire } from "node:module";

function fail(...messages) {
  writeSync(process.stderr.fd, `${messages.join("\n")}\n`);
  process.exit(2);
}

const targets = {
  "linux-x64": {
    packageName: "@benchguard/linux-x64",
    executableName: "benchguard",
  },
  "win32-x64": {
    packageName: "@benchguard/win32-x64",
    executableName: "benchguard.exe",
  },
};

const target = targets[`${process.platform}-${process.arch}`];

if (!target) {
  fail(
    `BenchGuard: Unsupported platform ${process.platform}-${process.arch}.`,
    "Supported npm targets: linux-x64, win32-x64.",
    "Download another available native binary from the GitHub release.",
  );
}

const require = createRequire(import.meta.url);
let executable;

try {
  executable = require.resolve(
    `${target.packageName}/${target.executableName}`,
  );
} catch {
  fail(
    `BenchGuard: Could not find ${target.packageName} for `
      + `${process.platform}-${process.arch}.`,
    "Reinstall @benchguard/cli with optional dependencies enabled, "
      + "or download the native binary from the GitHub release.",
  );
}
const result = spawnSync(executable, process.argv.slice(2), {
  stdio: "inherit",
});

if (result.error) {
  fail(
    `BenchGuard: Could not start the native binary: ${result.error.message}`,
  );
}

if (result.signal) {
  try {
    process.kill(process.pid, result.signal);
  } catch {
    fail(
      `BenchGuard: Native binary terminated with ${result.signal}, `
        + "but the launcher could not propagate that signal.",
    );
  }
}

if (!Number.isInteger(result.status)) {
  fail("BenchGuard: Native binary ended without an exit status.");
}

process.exit(result.status);
