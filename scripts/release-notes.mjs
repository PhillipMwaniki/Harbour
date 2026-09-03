#!/usr/bin/env node
// Prints the changelog section for one version, for use as a release body.
//
//   node scripts/release-notes.mjs 1.2.3 [CHANGELOG.md]
//
// The section must exist. Releasing from `[Unreleased]` would mean the notes
// a user reads on GitHub say "Unreleased" and change under them the next time
// anything lands; cutting the section is part of tagging, not an afterthought.

import { readFileSync } from "node:fs";

/** The body under `## [version]`, up to the next `## ` heading. */
export function sectionFor(changelog, version) {
  const lines = changelog.split(/\r?\n/);
  const start = lines.findIndex((line) => line.startsWith(`## [${version}]`));
  if (start === -1) return null;
  const end = lines.findIndex((line, index) => index > start && line.startsWith("## "));
  return lines
    .slice(start + 1, end === -1 ? undefined : end)
    .join("\n")
    .trim();
}

function main() {
  const version = process.argv[2];
  const file = process.argv[3] ?? "CHANGELOG.md";
  if (!version) {
    console.error("usage: release-notes.mjs <version> [CHANGELOG.md]");
    process.exit(1);
  }

  const body = sectionFor(readFileSync(file, "utf8"), version);
  if (body === null) {
    console.error(`${file} has no "## [${version}]" section.`);
    console.error("Move the [Unreleased] entries under a heading for this version, commit, and retag.");
    process.exit(1);
  }
  if (body === "") {
    console.error(`The "## [${version}]" section in ${file} is empty.`);
    process.exit(1);
  }

  process.stdout.write(`${body}\n`);
}

if (process.argv[1] && /release-notes\.mjs$/.test(process.argv[1])) {
  main();
}
