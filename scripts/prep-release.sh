#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage: scripts/prep-release.sh --version vX.Y.Z [--date YYYY-MM-DD] [--dry-run]

Mechanical release prep only:
  1. promote reviewed CHANGELOG.md [Unreleased] content to the target version
  2. leave a fresh empty [Unreleased] section at the top
  3. bump workspace.package.version in Cargo.toml

Draft release notes first if useful:
  /draft-release-notes <previous-tag>..<target-tag>
Then human-review/refine the text into CHANGELOG.md before running this script.
EOF
}

version=""
release_date="$(date -u +%F)"
dry_run=0
repo="."
skip_cargo_check=0

while [ "$#" -gt 0 ]; do
  case "$1" in
    --version)
      [ "$#" -ge 2 ] || { usage; exit 2; }
      version="$2"
      shift 2
      ;;
    --date)
      [ "$#" -ge 2 ] || { usage; exit 2; }
      release_date="$2"
      shift 2
      ;;
    --dry-run)
      dry_run=1
      shift
      ;;
    --repo)
      [ "$#" -ge 2 ] || { usage; exit 2; }
      repo="$2"
      shift 2
      ;;
    --skip-cargo-check)
      skip_cargo_check=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage
      exit 2
      ;;
  esac
done

if [ -z "$version" ]; then
  echo "missing required --version vX.Y.Z" >&2
  usage
  exit 2
fi

if ! [[ "$version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "--version must be shaped vX.Y.Z: $version" >&2
  exit 2
fi

if ! [[ "$release_date" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]]; then
  echo "--date must be shaped YYYY-MM-DD: $release_date" >&2
  exit 2
fi

repo="$(cd "$repo" && pwd)"
changelog="$repo/CHANGELOG.md"
cargo_toml="$repo/Cargo.toml"
package_version="${version#v}"

if [ -n "$(git -C "$repo" status --porcelain)" ]; then
  echo "release prep requires a clean git tree" >&2
  echo "commit or discard dirty workspace changes, then re-run prep-release.sh" >&2
  exit 3
fi

if [ -n "$(git -C "$repo" tag -l "$version")" ]; then
  echo "target tag already exists: $version" >&2
  echo "choose a new release tag or perform manual release-state repair" >&2
  exit 4
fi

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT
next_changelog="$tmpdir/CHANGELOG.md"
next_cargo="$tmpdir/Cargo.toml"

python3 - "$version" "$release_date" "$package_version" "$changelog" "$next_changelog" "$cargo_toml" "$next_cargo" <<'PY'
from __future__ import annotations

from pathlib import Path
import sys


version, release_date, package_version, changelog_path, next_changelog_path, cargo_path, next_cargo_path = sys.argv[1:]


def die(code: int, message: str) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(code)


changelog = Path(changelog_path).read_text(encoding="utf-8").splitlines()
try:
    start = changelog.index("## [Unreleased]")
except ValueError:
    die(5, "CHANGELOG.md has no ## [Unreleased] section")

end = len(changelog)
for idx in range(start + 1, len(changelog)):
    if changelog[idx].startswith("## ["):
        end = idx
        break

body = changelog[start + 1:end]
if not "\n".join(body).strip():
    die(
        6,
        "CHANGELOG.md [Unreleased] is empty. Invoke /draft-release-notes if useful, "
        "review/refine the suggested section into CHANGELOG.md, then re-run prep-release.sh.",
    )

next_changelog = (
    changelog[: start + 1]
    + [""]
    + [f"## [{version}] — {release_date}"]
    + body
    + changelog[end:]
)
Path(next_changelog_path).write_text("\n".join(next_changelog) + "\n", encoding="utf-8")

cargo_lines = Path(cargo_path).read_text(encoding="utf-8").splitlines()
in_workspace_package = False
replaced = False
for idx, line in enumerate(cargo_lines):
    if line == "[workspace.package]":
        in_workspace_package = True
        continue
    if in_workspace_package and line.startswith("[") and line.endswith("]"):
        break
    if in_workspace_package and line.startswith("version = "):
        cargo_lines[idx] = f'version = "{package_version}"'
        replaced = True
        break

if not replaced:
    die(5, "Cargo.toml has no version field under [workspace.package]")

Path(next_cargo_path).write_text("\n".join(cargo_lines) + "\n", encoding="utf-8")
PY

if [ "$dry_run" -eq 1 ]; then
  echo "prep-release dry run for $version ($release_date)"
  echo
  echo "CHANGELOG.md diff:"
  diff -u "$changelog" "$next_changelog" || true
  echo
  echo "Cargo.toml diff:"
  diff -u "$cargo_toml" "$next_cargo" || true
  exit 0
fi

mv "$next_changelog" "$changelog"
mv "$next_cargo" "$cargo_toml"

if [ "$skip_cargo_check" -eq 0 ]; then
  cargo check --workspace
fi

cat <<EOF
release prep complete for $version ($release_date)

Modified:
- CHANGELOG.md
- Cargo.toml

Next steps:
git add CHANGELOG.md Cargo.toml Cargo.lock
git commit -m "Release $version"
git tag -a $version -m "$version"
git push && git push --tags
EOF
