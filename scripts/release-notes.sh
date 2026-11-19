#!/usr/bin/env bash
# Print the GitHub Release body for VERSION: its CHANGELOG section followed
# by links to the published crates.
set -euo pipefail

cd "$(dirname "$0")/.."
source "$(dirname "$0")/crates.sh"

version="${1:?usage: release-notes.sh VERSION}"

notes="$(awk -v v="$version" '
  /^## / {
    if (found) exit
    if (substr($0, 4, length(v)) == v) found = 1
    next
  }
  found { print }
' CHANGELOG.md)"

if [ -z "$notes" ]; then
  echo "no CHANGELOG section for $version" >&2
  exit 1
fi

printf '%s\n' "$notes" | awk '
  { lines[NR] = $0 }
  END {
    last = NR
    while (last > 0 && lines[last] ~ /^[[:space:]]*$/) last--
    first = 1
    while (first <= last && lines[first] ~ /^[[:space:]]*$/) first++
    for (i = first; i <= last; i++) print lines[i]
  }
'

printf '\n## Crates\n\n'
for crate in "${CRATES[@]}"; do
  printf -- '- [%s %s](https://crates.io/crates/%s/%s)\n' \
    "$crate" "$version" "$crate" "$version"
done
