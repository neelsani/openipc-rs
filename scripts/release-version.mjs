#!/usr/bin/env node
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const rootDir = join(dirname(fileURLToPath(import.meta.url)), "..");

const tomlVersionFiles = [
  "crates/openipc-core/Cargo.toml",
  "crates/openipc-rtl88xx/Cargo.toml",
  "crates/openipc-native/Cargo.toml",
  "crates/openipc-web/Cargo.toml",
  "apps/openipc-station/src-tauri/Cargo.toml",
];

const packageJsonFiles = [
  "crates/openipc-web/package.json",
  "apps/openipc-station/package.json",
  "docs/package.json",
];

const localCargoPackages = new Set([
  "openipc-core",
  "openipc-rtl88xx",
  "openipc-native",
  "openipc-web",
  "openipc-rs-desktop",
]);

const localCargoDependencyNames = ["openipc-core", "openipc-rtl88xx"];

function usage() {
  console.error(`Usage:
  scripts/release-version.sh <version> [--push] [--dry-run] [--no-commit] [--no-tag]

Examples:
  scripts/release-version.sh 0.2.0 --dry-run
  scripts/release-version.sh 0.2.0
  scripts/release-version.sh 0.2.0 --push
  scripts/release-version.sh 0.2.0 --no-commit

Notes:
  - <version> can be "0.2.0" or "v0.2.0"; files are written without the leading "v".
  - By default the script writes version files, creates a release commit, and creates an annotated tag.
  - --dry-run previews file changes and never commits, tags, or pushes.
  - --no-commit writes files only and also disables tagging.
  - --no-tag creates the release commit but skips the annotated tag.
  - --push pushes the current branch and the created tag to origin.`);
}

function fail(message) {
  console.error(`release-version: ${message}`);
  process.exit(1);
}

const args = process.argv.slice(2);
if (args.length === 0 || args.includes("--help") || args.includes("-h")) {
  usage();
  process.exit(args.length === 0 ? 1 : 0);
}

const rawVersion = args[0];
const version = rawVersion.replace(/^v/, "");
const flags = new Set(args.slice(1));
const dryRun = flags.has("--dry-run");
const shouldPush = flags.has("--push");
const noCommit = flags.has("--no-commit");
const noTag = flags.has("--no-tag");
const explicitCommit = flags.has("--commit");
const explicitTag = flags.has("--tag");
const shouldCommit = !dryRun && !noCommit;
const shouldTag = shouldCommit && !noTag;
const tagName = `v${version}`;

for (const flag of flags) {
  if (
    ![
      "--commit",
      "--tag",
      "--push",
      "--dry-run",
      "--no-commit",
      "--no-tag",
    ].includes(flag)
  ) {
    fail(`unknown flag: ${flag}`);
  }
}
if (explicitCommit) {
  console.warn("release-version: --commit is now the default and can be omitted");
}
if (explicitTag) {
  console.warn("release-version: --tag is now the default and can be omitted");
}

if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(version)) {
  fail(`invalid SemVer version: ${rawVersion}`);
}
if (explicitCommit && noCommit) {
  fail("--commit and --no-commit cannot be used together");
}
if (explicitTag && noTag) {
  fail("--tag and --no-tag cannot be used together");
}
if (shouldPush && !shouldTag) {
  fail("--push requires tag creation; remove --dry-run, --no-commit, or --no-tag");
}

const changed = [];

function pathOf(file) {
  return join(rootDir, file);
}

function read(file) {
  return readFileSync(pathOf(file), "utf8");
}

function writeIfChanged(file, next) {
  const path = pathOf(file);
  const previous = existsSync(path) ? readFileSync(path, "utf8") : "";
  if (previous === next) {
    return;
  }
  changed.push(file);
  if (!dryRun) {
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, next);
  }
}

function updateTomlVersion(file) {
  let next = read(file).replace(/^version = ".*"$/m, `version = "${version}"`);
  for (const dependencyName of localCargoDependencyNames) {
    const escapedName = dependencyName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    next = next.replace(
      new RegExp(`^(${escapedName}\\s*=\\s*\\{[^\\n}]*version\\s*=\\s*")([^"]+)(")`, "gm"),
      `$1${version}$3`,
    );
  }
  writeIfChanged(file, next);
}

function updateJsonVersion(file) {
  const json = JSON.parse(read(file));
  json.version = version;
  writeIfChanged(file, `${JSON.stringify(json, null, 2)}\n`);
}

function updatePackageLock(file) {
  const json = JSON.parse(read(file));
  json.version = version;
  if (json.packages?.[""]) {
    json.packages[""].version = version;
  }
  writeIfChanged(file, `${JSON.stringify(json, null, 2)}\n`);
}

function updateCargoLock(file) {
  const lines = read(file).split("\n");
  let currentName = "";
  const next = lines
    .map((line) => {
      if (line === "[[package]]") {
        currentName = "";
        return line;
      }
      const name = line.match(/^name = "([^"]+)"$/);
      if (name) {
        currentName = name[1];
        return line;
      }
      if (currentName && localCargoPackages.has(currentName)) {
        return line.replace(/^version = ".*"$/, `version = "${version}"`);
      }
      return line;
    })
    .join("\n");
  writeIfChanged(file, next);
}

function updateTauriConfig(file) {
  updateJsonVersion(file);
}

for (const file of tomlVersionFiles) {
  updateTomlVersion(file);
}
for (const file of packageJsonFiles) {
  updateJsonVersion(file);
}
updatePackageLock("apps/openipc-station/package-lock.json");
if (existsSync(pathOf("docs/package-lock.json"))) {
  updatePackageLock("docs/package-lock.json");
}
updateTauriConfig("apps/openipc-station/src-tauri/tauri.conf.json");
updateCargoLock("Cargo.lock");

if (existsSync(pathOf("crates/openipc-web/pkg/package.json"))) {
  updateJsonVersion("crates/openipc-web/pkg/package.json");
}

if (changed.length === 0) {
  console.log(`All versioned files are already at ${version}.`);
} else if (dryRun) {
  console.log(`Would update ${changed.length} file(s) to ${version}:`);
  for (const file of changed) {
    console.log(`  ${file}`);
  }
} else {
  console.log(`Updated ${changed.length} file(s) to ${version}:`);
  for (const file of changed) {
    console.log(`  ${file}`);
  }
}

function git(args, options = {}) {
  return execFileSync("git", args, {
    cwd: rootDir,
    encoding: "utf8",
    stdio: options.stdio ?? "pipe",
  });
}

if (dryRun) {
  process.exit(0);
}

if (shouldCommit) {
  const filesToStage = changed.filter((file) => !file.startsWith("crates/openipc-web/pkg/"));
  if (filesToStage.length > 0) {
    git(["add", ...filesToStage], { stdio: "inherit" });
    git(["commit", "-m", `chore: release ${tagName}`], { stdio: "inherit" });
  } else {
    console.log("No source version files changed; skipping commit.");
  }
}

if (shouldTag) {
  let tagExists = false;
  try {
    git(["rev-parse", "--verify", tagName]);
    tagExists = true;
  } catch (error) {
    tagExists = false;
  }
  if (tagExists) {
    fail(`tag ${tagName} already exists`);
  }
  git(["tag", "-a", tagName, "-m", tagName], { stdio: "inherit" });
  console.log(`Created tag ${tagName}.`);
}

if (shouldPush) {
  const branch = git(["branch", "--show-current"]).trim();
  if (!branch) {
    fail("cannot push: current HEAD is detached");
  }
  git(["push", "origin", branch], { stdio: "inherit" });
  git(["push", "origin", tagName], { stdio: "inherit" });
}

if (changed.length > 0 && !shouldCommit) {
  console.log("");
  console.log("Next steps:");
  console.log(`  git diff -- ${changed.map((file) => relative(rootDir, pathOf(file))).join(" ")}`);
  console.log(`  git add ${changed.filter((file) => !file.startsWith("crates/openipc-web/pkg/")).join(" ")}`);
  console.log(`  git commit -m "chore: release ${tagName}"`);
  console.log(`  git tag -a ${tagName} -m "${tagName}"`);
}
