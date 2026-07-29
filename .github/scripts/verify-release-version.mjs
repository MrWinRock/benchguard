import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const repository = fileURLToPath(new URL("../..", import.meta.url));
const tag = process.argv[2] ?? process.env.GITHUB_REF_NAME;

assert.match(
  tag ?? "",
  /^v\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/,
  "release tag must be v followed by a semantic version",
);

const version = tag.slice(1);
const cargoManifest = readFileSync(`${repository}/Cargo.toml`, "utf8");
const cargoVersion = cargoManifest.match(
  /^\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m,
)?.[1];
assert.equal(cargoVersion, version, "Cargo package version must match the tag");

const packages = new Map();
for (const directory of ["cli", "linux-x64", "win32-x64"]) {
  const manifest = JSON.parse(
    readFileSync(`${repository}/npm/${directory}/package.json`, "utf8"),
  );
  assert.equal(
    manifest.version,
    version,
    `${manifest.name} version must match the tag`,
  );
  packages.set(manifest.name, manifest);
}

const cli = packages.get("@benchguard/cli");
for (const nativePackage of [
  "@benchguard/linux-x64",
  "@benchguard/win32-x64",
]) {
  assert.equal(
    cli.optionalDependencies[nativePackage],
    version,
    `${nativePackage} optional dependency must match the tag`,
  );
}

console.log(`release versions agree on ${version}`);
