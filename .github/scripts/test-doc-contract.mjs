import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  cpSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repository = resolve(fileURLToPath(new URL("../..", import.meta.url)));
const normalizeLineEndings = (value) => value.replace(/\r\n?/g, "\n");
const requiredFiles = [
  "README.md",
  "CHANGELOG.md",
  "CONTRIBUTING.md",
  "CODE_OF_CONDUCT.md",
  "LICENSE-MIT",
  "LICENSE-APACHE",
  "examples/benchguard.json",
  "docs/ci.md",
  "docs/json-format.md",
  "docs/releasing.md",
];

for (const relative of requiredFiles) {
  assert.ok(existsSync(join(repository, relative)), `missing release file ${relative}`);
}

for (const relative of [
  "README.md",
  "CONTRIBUTING.md",
  "docs/ci.md",
  "docs/json-format.md",
  "docs/releasing.md",
]) {
  const absolute = join(repository, relative);
  const markdown = readFileSync(absolute, "utf8");
  for (const match of markdown.matchAll(/\[[^\]]+\]\(([^)]+)\)/g)) {
    const target = match[1].split("#", 1)[0];
    if (!target || /^(?:https?:|mailto:)/.test(target)) continue;
    assert.ok(
      existsSync(resolve(dirname(absolute), target)),
      `${relative} links to missing local target ${target}`,
    );
  }
}

const jsonDocumentation = normalizeLineEndings(
  readFileSync(join(repository, "docs/json-format.md"), "utf8"),
);
const readmeDocumentation = normalizeLineEndings(
  readFileSync(join(repository, "README.md"), "utf8"),
);
const crlfReadmeFixture = normalizeLineEndings(
  readmeDocumentation.replaceAll("\n", "\r\n"),
);
assert.ok(
  crlfReadmeFixture.includes("The separator `--` is\noptional"),
  "README contract must accept Windows CRLF line endings",
);
for (const requiredText of [
  "benchguard record npm-build --runs 10 --max-time +10% npm run build",
  "benchguard record bun-build --runs 10 --max-time +10% bun run build",
  "The separator `--` is\noptional",
  "`--color auto`",
  "`NO_COLOR`",
  "JSON output is never colored and continues to use integer\nnanoseconds and bytes.",
]) {
  assert.ok(readmeDocumentation.includes(requiredText), `README.md is missing: ${requiredText}`);
}
const jsonBlocks = [...jsonDocumentation.matchAll(/```json\s+([\s\S]*?)```/g)];
assert.ok(jsonBlocks.length >= 2, "JSON documentation must include baseline and report examples");
const parsed = jsonBlocks.map(([, body]) => JSON.parse(body));
assert.ok(
  parsed.some((value) => value.schema_version === 1 && value.benchmarks?.startup),
  "JSON documentation is missing a schema-v1 baseline example",
);
assert.ok(
  parsed.some(
    (value) =>
      value.schema_version === 1 &&
      Array.isArray(value.benchmarks) &&
      Array.isArray(value.warnings) &&
      Array.isArray(value.errors),
  ),
  "JSON documentation is missing a stable report-envelope example",
);

const releasingDocumentation = readFileSync(join(repository, "docs/releasing.md"), "utf8");
const releaseJsonBlocks = [
  ...releasingDocumentation.matchAll(/```json\s+([\s\S]*?)```/g),
].map(([, body]) => JSON.parse(body));
const bootstrapAdmin = releaseJsonBlocks.find((value) => value.npm_bootstrap_admin)
  ?.npm_bootstrap_admin;
assert.deepEqual(
  bootstrapAdmin,
  {
    scope: {
      name: "@benchguard",
      ownership: "owned npm user or organization scope",
      packages_must_be_absent: true,
    },
    token: {
      resource_type: "scope",
      resource: "@benchguard",
      permission: "read-write",
      bypass_2fa: true,
      lifetime: "short-lived",
    },
    trusted_publisher: {
      provider: "GitHub Actions",
      workflow_filename: "release.yml",
      environment: "npm-production",
      allowed_actions: ["npm publish"],
    },
  },
  "release guide must define the safe first-publish and OIDC administration contract",
);

const suffix = process.platform === "win32" ? ".exe" : "";
const benchguard = join(repository, "target", "debug", `benchguard${suffix}`);
assert.ok(existsSync(benchguard), "build BenchGuard before validating the sample baseline");
const fixture = join(repository, "target", "debug", `benchguard-fixture${suffix}`);
assert.ok(existsSync(fixture), "build the fixture before validating documented output");

const exactHelpLines = new Map([
  [["help"], [
    "  record  Record or replace a performance baseline",
    "  check   Measure a command and check it against a stored baseline",
    "  list    List stored baselines without running commands",
    "  -V, --version        Print version",
  ]],
  [["help", "record"], [
    "  -r, --runs <RUNS>              Number of measured executions [default: 10]",
    "  -w, --warmup <WARMUP>          Number of unmeasured warm-up executions [default: 2]",
    "  -t, --timeout <TIMEOUT>        Maximum duration of each execution, such as 500ms or 2s",
    "  -f, --file <FILE>              Path to the baseline JSON file [default: benchguard.json]",
  ]],
  [["help", "check"], [
    "  [TARGET]...  Optional executable and arguments; omit to use the stored command",
    "  -r, --runs <RUNS>              Override the stored measured-run count",
    "  -f, --file <FILE>              Path to the baseline JSON file [default: benchguard.json]",
  ]],
  [["help", "list"], [
    "  -f, --file <FILE>      Path to the baseline JSON file [default: benchguard.json]",
    "      --format <FORMAT>  Report format [default: human] [possible values: human, json]",
  ]],
]);
for (const [args, expectedLines] of exactHelpLines) {
  const result = spawnSync(benchguard, args, { cwd: repository, encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  const actualLines = new Set(result.stdout.replaceAll("\r\n", "\n").split("\n"));
  for (const expected of expectedLines) {
    assert.ok(actualLines.has(expected), `help ${args.join(" ")} is missing exact line: ${expected}`);
  }
}

const sample = spawnSync(
  benchguard,
  ["list", "--format", "json", "--file", join(repository, "examples", "benchguard.json")],
  { cwd: repository, encoding: "utf8" },
);
assert.equal(sample.status, 0, sample.stderr);
const sampleReport = JSON.parse(sample.stdout);
assert.equal(sampleReport.schema_version, 1);
assert.equal(sampleReport.benchmarks[0].name, "startup");
assert.equal(sampleReport.benchmarks[0].status, "baseline");

const documentedTimeout = parsed.find((value) => value.status === "error");
assert.ok(documentedTimeout, "JSON documentation is missing an operational-error example");
const timeoutRoot = mkdtempSync(join(tmpdir(), "benchguard-doc-timeout-"));
try {
  const timeout = spawnSync(
    benchguard,
    [
      "record",
      "documented-timeout",
      "--runs",
      "1",
      "--warmup",
      "0",
      "--timeout",
      "1ms",
      "--format",
      "json",
      "--",
      fixture,
      "sleep-ms",
      "100",
    ],
    { cwd: timeoutRoot, encoding: "utf8" },
  );
  assert.equal(timeout.status, 2, timeout.stderr);
  assert.deepEqual(
    documentedTimeout,
    JSON.parse(timeout.stdout),
    "documented timeout JSON must exactly match the stable renderer",
  );
} finally {
  rmSync(timeoutRoot, { recursive: true, force: true });
}

const ciWorkflow = readFileSync(join(repository, ".github/workflows/ci.yml"), "utf8");
for (const command of [
  "node .github/scripts/test-doc-contract.mjs",
  "node scripts/test-readme-acceptance.mjs",
  "node scripts/readme-acceptance.mjs",
]) {
  assert.ok(ciWorkflow.includes(command), `CI does not run ${command}`);
}

const packRoot = mkdtempSync(join(tmpdir(), "benchguard-doc-pack-"));
process.once("exit", () => rmSync(packRoot, { recursive: true, force: true }));
const npmCache = join(packRoot, "cache");

for (const directory of ["cli", "linux-x64", "win32-x64"]) {
  for (const file of ["README.md", "LICENSE-MIT", "LICENSE-APACHE"]) {
    assert.ok(existsSync(join(repository, "npm", directory, file)), `npm/${directory}/${file} missing`);
  }
  const staged = join(packRoot, directory);
  cpSync(join(repository, "npm", directory), staged, { recursive: true });
  const native =
    directory === "linux-x64"
      ? "benchguard"
      : directory === "win32-x64"
        ? "benchguard.exe"
        : "bin/benchguard.js";
  if (directory !== "cli") {
    writeFileSync(join(staged, native), "release binary staged by the release workflow\n");
    if (directory === "linux-x64") chmodSync(join(staged, native), 0o755);
  }
  const packed = spawnSync("npm", ["pack", staged, "--dry-run", "--json"], {
    cwd: repository,
    encoding: "utf8",
    shell: process.platform === "win32",
    env: { ...process.env, npm_config_cache: npmCache },
  });
  assert.equal(packed.status, 0, packed.stderr);
  const manifest = JSON.parse(packed.stdout)[0];
  const files = new Set(manifest.files.map(({ path }) => path));
  for (const file of ["package.json", "README.md", "LICENSE-MIT", "LICENSE-APACHE"]) {
    assert.ok(files.has(file), `${manifest.name} package omits ${file}`);
  }
  assert.ok(files.has(native), `${manifest.name} package omits ${native}`);
}

rmSync(packRoot, { recursive: true, force: true });
process.removeAllListeners("exit");
