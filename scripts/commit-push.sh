#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

label="${1:-checkpoint}"

git add -A
git commit --allow-empty --quiet -m "$label"
git push --quiet origin main

printf 'Committed and pushed: %s.\n' "$label"
