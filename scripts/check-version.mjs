#!/usr/bin/env node
// Refuses to release a tag that disagrees with the code.
//
//   node scripts/check-version.mjs v1.2.3
//
// The version lives in three files - package.json, src-tauri/tauri.conf.json
// and src-tauri/Cargo.toml - and Tauri stamps the one in tauri.conf.json onto
// every installer. A tag that names a different version would produce
// installers claiming to be something they are not, so all three must match
// the tag exactly. On success the version and whether it is a pre-release are
// printed as `key=value` lines for the workflow to pick up.

import { readFileSync } from "node:fs";

const TAG = /^v(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)$/;

export function parseTag(tag) {
  const match = TAG.exec(tag ?? "");
  if (!match) return null;
  return { version: match[1], prerelease: match[1].includes("-") };
}

export function readVersions(root = ".") {
  const json = (path) => JSON.parse(readFileSync(`${root}/${path}`, "utf8")).version;
  const cargo = readFileSync(`${root}/src-tauri/Cargo.toml`, "utf8");
  return {
    "package.json": json("package.json"),
    "src-tauri/tauri.conf.json": json("src-tauri/tauri.conf.json"),
    "src-tauri/Cargo.toml": /^version\s*=\s*"([^"]+)"/m.exec(cargo)?.[1],
  };
}

/** Everything that disagrees with `version`, as human-readable lines. */
export function mismatches(version, versions) {
  return Object.entries(versions)
    .filter(([, found]) => found !== version)
    .map(([file, found]) => `${file} says ${found ?? "nothing"}, the tag says ${version}`);
}

function main() {
  const tag = process.argv[2];
  const parsed = parseTag(tag);
  if (!parsed) {
    console.error(`"${tag ?? ""}" is not a version tag. Expected vX.Y.Z, optionally with a -suffix for a pre-release.`);
    process.exit(1);
  }

  const problems = mismatches(parsed.version, readVersions());
  if (problems.length > 0) {
    console.error("The tag does not match the code:");
    for (const problem of problems) console.error(`  - ${problem}`);
    console.error("Bump every version field to match, commit, and retag.");
    process.exit(1);
  }

  process.stdout.write(`version=${parsed.version}\nprerelease=${parsed.prerelease}\n`);
}

if (process.argv[1] && /check-version\.mjs$/.test(process.argv[1])) {
  main();
}
