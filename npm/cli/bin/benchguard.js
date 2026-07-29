#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";

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
  console.error(
    `BenchGuard: Unsupported platform ${process.platform}-${process.arch}.`,
  );
  console.error("Supported npm targets: linux-x64, win32-x64.");
  console.error(
    "Download another available native binary from the GitHub release.",
  );
  process.exit(2);
}

const require = createRequire(import.meta.url);
let executable;

try {
  executable = require.resolve(
    `${target.packageName}/${target.executableName}`,
  );
} catch {
  console.error(
    `BenchGuard: Could not find ${target.packageName} for `
      + `${process.platform}-${process.arch}.`,
  );
  console.error(
    "Reinstall @benchguard/cli with optional dependencies enabled, "
      + "or download the native binary from the GitHub release.",
  );
  process.exit(2);
}
const result = spawnSync(executable, process.argv.slice(2), {
  stdio: "inherit",
});

if (result.error) {
  console.error(
    `BenchGuard: Could not start the native binary: ${result.error.message}`,
  );
  process.exit(2);
}

if (result.signal) {
  try {
    process.kill(process.pid, result.signal);
  } catch {
    console.error(
      `BenchGuard: Native binary terminated with ${result.signal}, `
        + "but the launcher could not propagate that signal.",
    );
    process.exit(2);
  }
}

if (!Number.isInteger(result.status)) {
  console.error("BenchGuard: Native binary ended without an exit status.");
  process.exit(2);
}

process.exit(result.status);
