#!/usr/bin/env node

import { copyFileSync, existsSync, mkdirSync, readdirSync, rmSync, statSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";

const bundleRoot = resolve(process.argv[2] ?? "apps/openipc-station/src-tauri/target");
const outputDir = resolve(process.argv[3] ?? "release-assets");
const releaseTag = process.env.RELEASE_TAG;
const assetLabel = process.env.ASSET_LABEL;

if (!releaseTag) {
  throw new Error("RELEASE_TAG is required");
}

if (!assetLabel) {
  throw new Error("ASSET_LABEL is required");
}

const suffixes = [
  [".app.tar.gz", ".app.tar.gz"],
  [".tar.gz", ".tar.gz"],
  [".appimage", ".AppImage"],
  [".dmg", ".dmg"],
  [".deb", ".deb"],
  [".rpm", ".rpm"],
  [".msi", ".msi"],
  [".exe", ".exe"],
];

function walk(dir) {
  if (!existsSync(dir)) {
    return [];
  }

  const entries = [];
  for (const name of readdirSync(dir)) {
    const file = join(dir, name);
    const stat = statSync(file);
    if (stat.isDirectory()) {
      entries.push(...walk(file));
    } else if (stat.isFile()) {
      entries.push(file);
    }
  }
  return entries;
}

function isBundleFile(file) {
  return file.split(/[\\/]/).includes("bundle");
}

function assetSuffix(file) {
  const lower = basename(file).toLowerCase();
  for (const [match, suffix] of suffixes) {
    if (lower.endsWith(match)) {
      return suffix;
    }
  }
  return null;
}

const bundleFiles = walk(bundleRoot)
  .filter(isBundleFile)
  .map((file) => [file, assetSuffix(file)])
  .filter(([, suffix]) => suffix !== null)
  .sort(([left], [right]) => left.localeCompare(right));

if (bundleFiles.length === 0) {
  throw new Error(`No Tauri bundle files found under ${bundleRoot}`);
}

rmSync(outputDir, { recursive: true, force: true });
mkdirSync(outputDir, { recursive: true });

const usedNames = new Map();
for (const [source, suffix] of bundleFiles) {
  const baseName = `OpenIPC-Station-${releaseTag}-${assetLabel}`;
  const seen = usedNames.get(suffix) ?? 0;
  usedNames.set(suffix, seen + 1);

  const counter = seen === 0 ? "" : `-${seen + 1}`;
  const targetName = `${baseName}${counter}${suffix}`;
  const target = join(outputDir, targetName);

  mkdirSync(dirname(target), { recursive: true });
  copyFileSync(source, target);
  console.log(`${source} -> ${target}`);
}
