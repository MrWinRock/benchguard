import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { validatePerformanceCheck } from "./readme-acceptance-contract.mjs";

const repository = resolve(fileURLToPath(new URL("..", import.meta.url)));
const readme = join(repository, "README.md");
assert.ok(existsSync(readme), "README.md must exist before its quick start can be accepted");

const source = readFileSync(readme, "utf8");
const documentedCommands = [
  "benchguard record startup --runs 10 --max-time +10% -- my-app --version",
  "benchguard check startup",
  "benchguard list",
  "benchguard help record",
];
for (const command of documentedCommands) {
  assert.ok(source.includes(command), `README.md is missing quick-start command: ${command}`);
}

const executableSuffix = process.platform === "win32" ? ".exe" : "";
const benchguardSource = join(repository, "target", "debug", `benchguard${executableSuffix}`);
const exampleAppSource = join(
  repository,
  "target",
  "debug",
  `benchguard-fixture${executableSuffix}`,
);
assert.ok(
  existsSync(benchguardSource),
  `build the debug binary before running this acceptance test: ${benchguardSource}`,
);
assert.ok(
  existsSync(exampleAppSource),
  `build the workspace before running this acceptance test: ${exampleAppSource}`,
);

const project = mkdtempSync(join(tmpdir(), "benchguard-readme-"));
process.once("exit", () => rmSync(project, { recursive: true, force: true }));
const benchguard = join(project, `benchguard${executableSuffix}`);
const exampleApp = join(project, `my-app${executableSuffix}`);
copyFileSync(benchguardSource, benchguard);
copyFileSync(exampleAppSource, exampleApp);
if (process.platform !== "win32") {
  chmodSync(benchguard, 0o755);
  chmodSync(exampleApp, 0o755);
}

const environment = {
  ...process.env,
  PATH: `${project}${delimiter}${process.env.PATH ?? ""}`,
};
const run = (args) =>
  spawnSync(benchguard, args, {
    cwd: project,
    env: environment,
    encoding: "utf8",
  });

const record = run([
  "record",
  "startup",
  "--runs",
  "10",
  "--max-time",
  "+10%",
  "--",
  "my-app",
  "--version",
]);
assert.equal(record.status, 0, record.stderr);
assert.match(record.stdout, /RECORDED/);

const check = run(["check", "startup"]);
validatePerformanceCheck(check);

const list = run(["list"]);
assert.equal(list.status, 0, list.stderr);
assert.match(list.stdout, /startup/);
assert.match(list.stdout, /BASELINE/);

const help = run(["help", "record"]);
assert.equal(help.status, 0, help.stderr);
for (const option of ["-r, --runs", "-w, --warmup", "-t, --timeout", "-f, --file"]) {
  assert.match(help.stdout, new RegExp(option.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")));
}

rmSync(project, { recursive: true, force: true });
process.removeAllListeners("exit");
