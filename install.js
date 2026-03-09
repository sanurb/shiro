#!/usr/bin/env node
"use strict";

// Downloads the correct shiro binary from GitHub Releases during npm postinstall.
// Mirrors the platform detection in install.sh but runs in Node.js for npm installs.

const { execSync } = require("child_process");
const fs = require("fs");
const path = require("path");
const https = require("https");
const { createWriteStream } = require("fs");
const { pipeline } = require("stream/promises");

const REPO = "sanurb/shiro";
const VERSION = require("./package.json").version;
const TAG = `v${VERSION}`;

function detectTarget() {
  const platform = process.platform;
  const arch = process.arch;

  if (platform === "linux" && arch === "x64") return "x86_64-unknown-linux-gnu";
  if (platform === "linux" && arch === "arm64") return "aarch64-unknown-linux-gnu";
  if (platform === "darwin" && arch === "x64") return "x86_64-apple-darwin";
  if (platform === "darwin" && arch === "arm64") return "aarch64-apple-darwin";

  throw new Error(
    `Unsupported platform: ${platform}-${arch}. ` +
      `Install from source: cargo install shiro-cli`
  );
}

function fetch(url) {
  return new Promise((resolve, reject) => {
    https
      .get(url, { headers: { "User-Agent": "shiro-cli-npm-install" } }, (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          return fetch(res.headers.location).then(resolve, reject);
        }
        if (res.statusCode !== 200) {
          return reject(new Error(`HTTP ${res.statusCode} for ${url}`));
        }
        resolve(res);
      })
      .on("error", reject);
  });
}

async function main() {
  const target = detectTarget();
  const archive = `shiro-cli-${TAG}-${target}`;
  const tarball = `${archive}.tar.gz`;
  const url = `https://github.com/${REPO}/releases/download/${TAG}/${tarball}`;

  const binDir = path.join(__dirname, "bin");
  fs.mkdirSync(binDir, { recursive: true });

  const tmpFile = path.join(binDir, tarball);

  console.log(`shiro: downloading ${tarball}...`);

  const res = await fetch(url);
  const writeStream = createWriteStream(tmpFile);
  await pipeline(res, writeStream);

  // Extract the binary from the tarball
  execSync(`tar xzf "${tmpFile}" -C "${binDir}" --strip-components=1 "${archive}/shiro"`, {
    stdio: "inherit",
  });

  // Clean up tarball
  fs.unlinkSync(tmpFile);

  // Make executable
  const binPath = path.join(binDir, "shiro");
  fs.chmodSync(binPath, 0o755);

  console.log(`shiro: installed to ${binPath}`);
}

main().catch((err) => {
  console.error(`shiro: failed to install binary: ${err.message}`);
  console.error("shiro: install manually via: cargo install shiro-cli");
  process.exit(1);
});
