#!/usr/bin/env node
// Télécharge un ffmpeg statique et l'installe en src-tauri/binaries/ffmpeg-<triple>[.exe]
// (sidecar Tauri). Node >= 20, aucune dépendance npm.
//
// Usage : node scripts/fetch-ffmpeg.mjs [--target <triple>] [--from-path] [--force]

import { execFileSync } from "node:child_process";
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const BTBN = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest";
const SOURCES = {
  "x86_64-pc-windows-msvc": { url: `${BTBN}/ffmpeg-master-latest-win64-gpl.zip`, archive: "zip", bin: "ffmpeg.exe" },
  "x86_64-unknown-linux-gnu": { url: `${BTBN}/ffmpeg-master-latest-linux64-gpl.tar.xz`, archive: "tar.xz", bin: "ffmpeg" },
  "aarch64-apple-darwin": { url: "https://ffmpeg.martin-riedl.de/redirect/latest/macos/arm64/release/ffmpeg.zip", archive: "zip", bin: "ffmpeg" },
  "x86_64-apple-darwin": { url: "https://ffmpeg.martin-riedl.de/redirect/latest/macos/amd64/release/ffmpeg.zip", archive: "zip", bin: "ffmpeg" },
};

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = join(root, "src-tauri", "binaries");

function parseArgs(argv) {
  const opts = { target: null, fromPath: false, force: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--target") opts.target = argv[++i];
    else if (a.startsWith("--target=")) opts.target = a.slice(9);
    else if (a === "--from-path") opts.fromPath = true;
    else if (a === "--force") opts.force = true;
    else die(`Argument inconnu : ${a}`);
  }
  return opts;
}

function die(msg) {
  console.error(`Erreur : ${msg}`);
  process.exit(1);
}

function hostTriple() {
  try {
    const out = execFileSync("rustc", ["-vV"], { encoding: "utf8" });
    const m = out.match(/^host:\s*(\S+)/m);
    if (m) return m[1];
  } catch {}
  die("impossible de déterminer la cible (rustc introuvable). Utilise --target <triple>.");
}

function findInPath(name) {
  const cmd = process.platform === "win32" ? "where" : "which";
  try {
    const out = execFileSync(cmd, [name], { encoding: "utf8" });
    const first = out.split(/\r?\n/).find(Boolean);
    if (first) return first.trim();
  } catch {}
  return null;
}

function findRecursive(dir, fileName) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const p = join(dir, entry.name);
    if (entry.isDirectory()) {
      const r = findRecursive(p, fileName);
      if (r) return r;
    } else if (entry.name.toLowerCase() === fileName.toLowerCase()) {
      return p;
    }
  }
  return null;
}

async function download(url, dest) {
  console.log(`Téléchargement : ${url}`);
  const res = await fetch(url, { redirect: "follow" });
  if (!res.ok) throw new Error(`HTTP ${res.status} ${res.statusText}`);
  const total = Number(res.headers.get("content-length")) || 0;
  const chunks = [];
  let got = 0;
  let lastPct = -10;
  for await (const chunk of res.body) {
    chunks.push(chunk);
    got += chunk.length;
    if (total) {
      const pct = Math.floor((got / total) * 100);
      if (pct >= lastPct + 10) {
        lastPct = pct;
        console.log(`  ${pct}% (${(got / 1048576).toFixed(0)} Mo)`);
      }
    }
  }
  writeFileSync(dest, Buffer.concat(chunks));
  console.log(`  ${(got / 1048576).toFixed(1)} Mo reçus.`);
}

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  const target = opts.target || hostTriple();
  const isWin = target.includes("windows");
  const dest = join(outDir, `ffmpeg-${target}${isWin ? ".exe" : ""}`);

  if (existsSync(dest) && !opts.force) {
    console.log(`ffmpeg déjà présent : ${dest} (utilise --force pour le remplacer).`);
    return;
  }
  mkdirSync(outDir, { recursive: true });

  if (opts.fromPath) {
    const found = findInPath("ffmpeg");
    if (!found) die("ffmpeg est introuvable dans le PATH.");
    console.log(`Copie de ${found}`);
    copyFileSync(found, dest);
  } else {
    const src = SOURCES[target];
    if (!src) die(`cible non prise en charge : ${target}. Cibles : ${Object.keys(SOURCES).join(", ")}.`);
    const tmp = mkdtempSync(join(tmpdir(), "ffmpeg-fetch-"));
    try {
      const archive = join(tmp, `ffmpeg.${src.archive}`);
      await download(src.url, archive);
      const extractDir = join(tmp, "extract");
      mkdirSync(extractDir);
      console.log("Extraction…");
      // Windows : tar.exe système (bsdtar, gère .zip) plutôt que le tar GNU de Git Bash.
      // Chemins relatifs + cwd pour éviter que "C:" soit lu comme un hôte distant.
      const sysTar = process.platform === "win32" ? join(process.env.SystemRoot || "C:\\Windows", "System32", "tar.exe") : "tar";
      execFileSync(existsSync(sysTar) ? sysTar : "tar", ["-xf", `ffmpeg.${src.archive}`, "-C", "extract"], { stdio: "inherit", cwd: tmp });
      const bin = findRecursive(extractDir, src.bin);
      if (!bin) die(`${src.bin} introuvable dans l'archive.`);
      copyFileSync(bin, dest);
    } finally {
      rmSync(tmp, { recursive: true, force: true });
    }
  }

  if (!isWin) chmodSync(dest, 0o755);
  console.log(`ffmpeg installé : ${dest} (${(statSync(dest).size / 1048576).toFixed(1)} Mo)`);
}

main().catch((e) => die(e.message || String(e)));
