#!/usr/bin/env node
import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const version = process.argv[2];
const semverPattern =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;

if (!version || !semverPattern.test(version)) {
  console.error("Usage: vp run version:set <semver>");
  console.error("Example: vp run version:set 0.1.18");
  process.exit(1);
}

async function updateTextFile(path, updater) {
  const fullPath = resolve(root, path);
  const previous = await readFile(fullPath, "utf8");
  const next = updater(previous, path);

  if (next !== previous) {
    await writeFile(fullPath, next);
  }
}

async function updateJsonVersion(path) {
  await updateTextFile(path, (text) => {
    JSON.parse(text);

    return replaceExactlyOnce(
      text,
      /^(\s*"version":\s*)"[^"]+"/m,
      `$1"${version}"`,
      path
    );
  });
}

function replaceExactlyOnce(text, pattern, replacement, path) {
  const flags = pattern.flags.includes("g")
    ? pattern.flags
    : `${pattern.flags}g`;
  const matches = [...text.matchAll(new RegExp(pattern.source, flags))];

  if (!matches || matches.length !== 1) {
    throw new Error(`${path}: expected exactly one match for ${pattern}`);
  }

  return text.replace(pattern, replacement);
}

function updateCargoPackageVersion(text, path) {
  return replaceExactlyOnce(
    text,
    /(^\[package\][\s\S]*?^version\s*=\s*)"[^"]+"/m,
    `$1"${version}"`,
    path
  );
}

function updateCargoLockPackageVersion(text, packageName, path) {
  return replaceExactlyOnce(
    text,
    new RegExp(
      `(^name = "${escapeRegex(packageName)}"\\nversion = )"[^"]+"`,
      "m"
    ),
    `$1"${version}"`,
    path
  );
}

function escapeRegex(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

await Promise.all([
  updateJsonVersion("mobile/app.json"),
  updateJsonVersion("mobile/modules/stereodrome-core/package.json"),
  updateTextFile("Cargo.lock", (text, path) =>
    [
      "stereodrome",
      "stereodrome-core",
      "stereodrome-ffi",
      "stereodrome-audio",
    ].reduce(
      (next, packageName) =>
        updateCargoLockPackageVersion(next, packageName, path),
      text
    )
  ),
  updateTextFile(
    "crates/stereodrome-core/Cargo.toml",
    updateCargoPackageVersion
  ),
  updateTextFile(
    "crates/stereodrome-ffi/Cargo.toml",
    updateCargoPackageVersion
  ),
  updateTextFile(
    "crates/stereodrome-audio/Cargo.toml",
    updateCargoPackageVersion
  ),
  updateTextFile("src-tauri/Cargo.toml", updateCargoPackageVersion),
  updateTextFile(
    "mobile/modules/stereodrome-core/android/build.gradle",
    (text, path) => {
      const withProjectVersion = replaceExactlyOnce(
        text,
        /^version = '[^']+'/m,
        `version = '${version}'`,
        path
      );

      return replaceExactlyOnce(
        withProjectVersion,
        /^\s*versionName "[^"]+"/m,
        `    versionName "${version}"`,
        path
      );
    }
  ),
]);

console.log(`Set Stereodrome version to ${version}`);
