#!/usr/bin/env node
import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { copyFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const crateDir = dirname(fileURLToPath(import.meta.url));
const rootDir = resolve(crateDir, "../..");
const pkgDir = join(crateDir, "pkg");

function usage() {
  console.error(`Usage:
  bun run --cwd crates/openipc-web build

Builds openipc-web for wasm32-unknown-unknown, generates wasm-bindgen
JavaScript/TypeScript glue into crates/openipc-web/pkg, and copies publishable
npm package metadata plus the MIT license into that generated package directory.

Set OPENIPC_RS_USE_WASM_PACK=1 to use wasm-pack when it is installed.`);
}

function run(command, args, options = {}) {
  execFileSync(command, args, {
    cwd: options.cwd ?? rootDir,
    stdio: options.stdio ?? "inherit",
    encoding: "utf8",
  });
}

function output(command, args, options = {}) {
  return execFileSync(command, args, {
    cwd: options.cwd ?? rootDir,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
}

function commandExists(command) {
  const result = spawnSync(command, ["--version"], {
    cwd: rootDir,
    stdio: "ignore",
    shell: process.platform === "win32",
  });
  return result.status === 0;
}

function readWasmBindgenVersion() {
  const lock = readFileSync(join(rootDir, "Cargo.lock"), "utf8").split(/\r?\n/);
  let inPackage = false;
  for (const line of lock) {
    if (line === "[[package]]") {
      inPackage = false;
      continue;
    }
    if (line === 'name = "wasm-bindgen"') {
      inPackage = true;
      continue;
    }
    if (inPackage) {
      const version = line.match(/^version = "([^"]+)"$/);
      if (version) {
        return version[1];
      }
    }
  }
  throw new Error("Could not determine wasm-bindgen version from Cargo.lock");
}

function wasmBindgenBinary() {
  const executable = process.platform === "win32" ? "wasm-bindgen.exe" : "wasm-bindgen";
  return join(rootDir, ".cargo-tools", "bin", executable);
}

function installedWasmBindgenVersion(binary) {
  if (!existsSync(binary)) {
    return "";
  }
  try {
    return output(binary, ["--version"]).trim().split(/\s+/)[1] ?? "";
  } catch {
    return "";
  }
}

async function copyPackageMetadata() {
  mkdirSync(pkgDir, { recursive: true });

  const packageJson = JSON.parse(readFileSync(join(crateDir, "package.json"), "utf8"));
  delete packageJson.scripts;
  writeFileSync(join(pkgDir, "package.json"), `${JSON.stringify(packageJson, null, 2)}\n`);

  await copyFile(join(crateDir, "README.md"), join(pkgDir, "README.md"));
  await copyFile(join(rootDir, "LICENSE"), join(pkgDir, "LICENSE"));
}

function ensureWasmTarget() {
  const installedTargets = output("rustup", ["target", "list", "--installed"]);
  if (!installedTargets.split(/\r?\n/).includes("wasm32-unknown-unknown")) {
    console.log("Installing Rust target wasm32-unknown-unknown...");
    run("rustup", ["target", "add", "wasm32-unknown-unknown"]);
  }
}

async function main() {
  const arg = process.argv[2] ?? "";
  if (arg === "--help" || arg === "-h") {
    usage();
    return;
  }
  if (arg) {
    console.error(`unknown argument: ${arg}`);
    usage();
    process.exit(1);
  }

  ensureWasmTarget();

  if (process.env.OPENIPC_RS_USE_WASM_PACK === "1" && commandExists("wasm-pack")) {
    run("wasm-pack", ["build", crateDir, "--target", "web", "--out-dir", "pkg"]);
    await copyPackageMetadata();
    return;
  }

  const expectedVersion = readWasmBindgenVersion();
  const wasmBindgen = wasmBindgenBinary();
  if (installedWasmBindgenVersion(wasmBindgen) !== expectedVersion) {
    run("cargo", [
      "install",
      "wasm-bindgen-cli",
      "--version",
      expectedVersion,
      "--root",
      join(rootDir, ".cargo-tools"),
    ]);
  }

  run("cargo", ["build", "-p", "openipc-web", "--target", "wasm32-unknown-unknown", "--release"]);
  run(wasmBindgen, [
    join(rootDir, "target", "wasm32-unknown-unknown", "release", "openipc_web.wasm"),
    "--target",
    "web",
    "--out-dir",
    pkgDir,
    "--typescript",
  ]);
  await copyPackageMetadata();
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
});
