#!/usr/bin/env bash
# Sets the app version everywhere it is stored.
#   scripts/bump-version.sh 0.2.0
# Then: add a CHANGELOG.md section, commit, and tag:  git tag v0.2.0 && git push --tags
set -euo pipefail
cd "$(dirname "$0")/.."
v="${1:?usage: scripts/bump-version.sh X.Y.Z}"
[[ "$v" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] || { echo "not a version: $v" >&2; exit 1; }

node -e '
  const fs = require("fs");
  for (const f of ["package.json", "src-tauri/tauri.conf.json"]) {
    const j = JSON.parse(fs.readFileSync(f, "utf8"));
    j.version = process.argv[1];
    fs.writeFileSync(f, JSON.stringify(j, null, 2) + "\n");
  }
  const lock = JSON.parse(fs.readFileSync("package-lock.json", "utf8"));
  lock.version = process.argv[1];
  if (lock.packages && lock.packages[""]) lock.packages[""].version = process.argv[1];
  fs.writeFileSync("package-lock.json", JSON.stringify(lock, null, 2) + "\n");
' "$v"
# First `version = ` in Cargo.toml is the package version.
perl -0pi -e "s/^version = \"[^\"]*\"/version = \"$v\"/m" src-tauri/Cargo.toml
# The tidings entry in Cargo.lock.
perl -0pi -e "s/(name = \"tidings\"\nversion = )\"[^\"]*\"/\$1\"$v\"/" src-tauri/Cargo.lock
grep -H '"version"' package.json src-tauri/tauri.conf.json | head -2
grep -H '^version' src-tauri/Cargo.toml
