import {
  copyFile,
  mkdir,
  readdir,
  readFile,
  writeFile,
} from "node:fs/promises";
import { basename, join } from "node:path";

const [inputDir, outputDir, repository, version] = process.argv.slice(2);
if (!inputDir || !outputDir || !repository || !version) {
  throw new Error(
    "Usage: generate-update-manifest.mjs <input-dir> <output-dir> <owner/repo> <version>"
  );
}

await mkdir(outputDir, { recursive: true });
const files = await walk(inputDir);
const platforms = {};

for (const source of files.filter((path) => !path.endsWith(".sig"))) {
  const name = basename(source);
  const releaseName = name;
  await copyFile(source, join(outputDir, releaseName));

  const signaturePath = `${source}.sig`;
  if (!files.includes(signaturePath)) continue;
  const signature = (await readFile(signaturePath, "utf8")).trim();
  await copyFile(signaturePath, join(outputDir, `${releaseName}.sig`));
  const url = `https://github.com/${repository}/releases/download/v${version}/${encodeURIComponent(releaseName)}`;

  if (name.endsWith(".app.tar.gz")) {
    const release = { signature, url, format: "app" };
    platforms["macos-aarch64"] = release;
    platforms["macos-x86_64"] = release;
  } else if (name.endsWith(".AppImage.tar.gz")) {
    platforms["linux-x86_64"] = { signature, url, format: "appimage" };
  } else if (name.endsWith(".nsis.zip")) {
    platforms["windows-x86_64"] = { signature, url, format: "nsis" };
  }
}

for (const target of [
  "macos-aarch64",
  "macos-x86_64",
  "linux-x86_64",
  "windows-x86_64",
]) {
  if (!platforms[target])
    throw new Error(`Missing signed updater artifact for ${target}`);
}

await writeFile(
  join(outputDir, "latest.json"),
  `${JSON.stringify({ version, platforms }, null, 2)}\n`
);

async function walk(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const paths = await Promise.all(
    entries.map((entry) => {
      const path = join(directory, entry.name);
      return entry.isDirectory() ? walk(path) : path;
    })
  );
  return paths.flat().sort();
}
