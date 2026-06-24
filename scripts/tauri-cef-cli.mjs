#!/usr/bin/env node

import { existsSync, readdirSync } from "node:fs";
import { spawn, spawnSync } from "node:child_process";
import os from "node:os";
import path from "node:path";

const args = process.argv.slice(2);
const overrideManifest = process.env.STEREODROME_TAURI_CLI_MANIFEST;
const isMacDev = process.platform === "darwin" && args[0] === "dev";
const devExecutable = path.join(
  process.cwd(),
  "target",
  "debug",
  "bundle",
  "macos",
  "Stereodrome.app",
  "Contents",
  "MacOS",
  "stereodrome"
);

function isCefCliManifest(manifestPath) {
  const cliDir = path.dirname(manifestPath);
  return existsSync(path.join(cliDir, "src", "cef", "macos_dev.rs"));
}

function findTauriCliManifest() {
  if (overrideManifest) {
    return isCefCliManifest(overrideManifest) ? overrideManifest : null;
  }

  const checkoutsDir = path.join(os.homedir(), ".cargo", "git", "checkouts");
  if (!existsSync(checkoutsDir)) {
    return null;
  }

  for (const checkoutName of readdirSync(checkoutsDir)) {
    if (!checkoutName.startsWith("tauri-")) {
      continue;
    }

    const checkoutDir = path.join(checkoutsDir, checkoutName);
    for (const revisionName of readdirSync(checkoutDir)) {
      const manifestPath = path.join(
        checkoutDir,
        revisionName,
        "crates",
        "tauri-cli",
        "Cargo.toml"
      );
      if (existsSync(manifestPath) && isCefCliManifest(manifestPath)) {
        return manifestPath;
      }
    }
  }

  return null;
}

function ensureTauriCheckout() {
  const result = spawnSync("cargo", ["fetch"], {
    cwd: process.cwd(),
    stdio: "inherit",
  });

  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

function pidsForDevExecutable() {
  if (!existsSync(devExecutable)) {
    return [];
  }

  const result = spawnSync("pgrep", ["-f", devExecutable], {
    encoding: "utf8",
  });

  if (result.status !== 0 && result.status !== 1) {
    return [];
  }

  return result.stdout
    .split(/\s+/)
    .filter(Boolean)
    .map((pid) => Number(pid))
    .filter((pid) => Number.isInteger(pid) && pid > 0 && pid !== process.pid);
}

function terminateDevApp(signal = "SIGTERM") {
  for (const pid of pidsForDevExecutable()) {
    try {
      process.kill(pid, signal);
    } catch (error) {
      if (error.code !== "ESRCH") {
        console.warn(
          `Failed to send ${signal} to stale Stereodrome dev process ${pid}: ${error}`
        );
      }
    }
  }
}

if (isMacDev) {
  terminateDevApp();
}

let manifestPath = findTauriCliManifest();
if (!manifestPath) {
  ensureTauriCheckout();
  manifestPath = findTauriCliManifest();
}

if (!manifestPath) {
  console.error(
    "Could not find a CEF-aware Tauri CLI checkout. Run `cargo fetch` and make sure the Tauri feat/cef git dependency is available."
  );
  process.exit(1);
}

const child = spawn(
  "cargo",
  ["run", "--manifest-path", manifestPath, "--", ...args],
  {
    cwd: process.cwd(),
    env: process.env,
    stdio: "inherit",
  }
);

function forwardSignal(signal) {
  if (isMacDev) {
    terminateDevApp();
  }

  child.kill(signal);
}

process.once("SIGINT", () => forwardSignal("SIGINT"));
process.once("SIGTERM", () => forwardSignal("SIGTERM"));

child.on("exit", (code, signal) => {
  if (isMacDev) {
    terminateDevApp();
  }

  if (signal) {
    process.kill(process.pid, signal);
    return;
  }

  process.exit(code ?? 1);
});
