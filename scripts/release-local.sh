#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/release-local.sh [options]

Run the local-first RMUX packaging + verification + signing/metadata dry-run
pipeline on Linux or macOS.

Options:
  --platform linux|macos       Platform override (default: host detection)
  --configuration debug|release
                               Cargo profile to package (default: release)
  -h, --help                   Show this help
USAGE
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  elif command -v openssl >/dev/null 2>&1; then
    openssl dgst -sha256 "$1" | awk '{print $NF}'
  else
    die "no SHA256 tool found"
  fi
}

json_escape() {
  sed 's/\\/\\\\/g; s/"/\\"/g'
}

display_path() {
  local path full
  path="$1"
  case "$path" in
    "<repo>"/*) printf '%s\n' "${path#"<repo>/"}"; return 0 ;;
  esac
  case "$path" in
    /*) full="$path" ;;
    *) full="$repo_root/$path" ;;
  esac
  case "$full" in
    "$repo_root"/*) printf '%s\n' "${full#"$repo_root"/}" ;;
    *) printf '%s\n' "$full" ;;
  esac
}

redact_log() {
  local file tmp
  file="$1"
  [ -f "$file" ] || return 0
  tmp="$file.tmp"
  sed "s|$repo_root|<repo>|g" "$file" > "$tmp"
  mv "$tmp" "$file"
}

run_logged() {
  local log
  log="$1"
  shift
  set +e
  "$@" > "$log" 2>&1
  local status=$?
  set -e
  redact_log "$log"
  return "$status"
}

write_checksums() {
  local root output file
  root="$1"
  output="$2"
  (
    cd "$root"
    find . -type f ! -path './SHA256SUMS.txt' | LC_ALL=C sort |
      while IFS= read -r file; do
        printf '%s  %s\n' "$(sha256_file "$file")" "${file#./}"
      done
  ) > "$output"
}

kv_value() {
  local key file
  key="$1"
  file="$2"
  sed -n "s/^$key=//p" "$file" | tail -n 1
}

resolve_logged_path() {
  local path
  path="$1"
  case "$path" in
    "<repo>"/*) printf '%s\n' "$repo_root/${path#"<repo>/"}" ;;
    *) printf '%s\n' "$path" ;;
  esac
}

detect_platform() {
  case "$(uname -s)" in
    Linux) printf 'linux\n' ;;
    Darwin) printf 'macos\n' ;;
    *) die "unsupported Unix host: $(uname -s)" ;;
  esac
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
platform=""
configuration="release"

while [ "$#" -gt 0 ]; do
  case "$1" in
    --platform)
      [ "$#" -ge 2 ] || die "--platform requires a value"
      platform="$2"
      shift 2
      ;;
    --configuration)
      [ "$#" -ge 2 ] || die "--configuration requires a value"
      configuration="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

[ "$configuration" = "debug" ] || [ "$configuration" = "release" ] || die "unsupported configuration: $configuration"
if [ -z "$platform" ]; then
  platform="$(detect_platform)"
fi
case "$platform" in linux|macos) ;; *) die "unsupported platform: $platform" ;; esac

cd "$repo_root"
git_status_before="$(git status --short --branch)"
if [ -n "$(git status --porcelain)" ]; then
  printf '%s\n' "$git_status_before" >&2
  die "release-local requires a clean worktree before generating reference"
fi

head_sha="$(git rev-parse HEAD)"
short_sha="$(git rev-parse --short=12 HEAD)"
dist_dir="target/dist/release-automation-$platform-$short_sha"
tmp_root="target/release-automation-logs/$platform-$short_sha"
reference_root="target/release-reference/artifacts/release-automation-$platform-$short_sha"
manifest_path="target/release-reference/release_automation_${platform}-$short_sha.txt"

rm -rf "$tmp_root" "$reference_root"
mkdir -p "$tmp_root/logs"

{
  printf 'git_status_before_reference:\n%s\n' "$git_status_before"
  printf 'git_head=%s\n' "$head_sha"
  printf 'platform=%s\n' "$platform"
  printf 'configuration=%s\n' "$configuration"
  printf 'uname=%s\n' "$(uname -a)"
  printf 'rustc=%s\n' "$(rustc --version 2>/dev/null || printf unavailable)"
  printf 'cargo=%s\n' "$(cargo --version 2>/dev/null || printf unavailable)"
} > "$tmp_root/logs/preflight.log"

package_log="$tmp_root/logs/package.log"
run_logged "$package_log" \
  "$repo_root/scripts/package-unix.sh" \
  --configuration "$configuration" \
  --output-dir "$dist_dir"

package_path="$(resolve_logged_path "$(kv_value package "$package_log")")"
package_sha="$(kv_value sha256 "$package_log")"
[ -n "$package_path" ] || die "package script did not emit package=<path>"
[ -n "$package_sha" ] || die "package script did not emit sha256=<hash>"

verify_log="$tmp_root/logs/verify-package.log"
run_logged "$verify_log" \
  "$repo_root/scripts/verify-package.sh" \
  "$package_path" \
  --checksums "$dist_dir/SHA256SUMS.txt" \
  --run-binary

signing_log="$tmp_root/logs/signing-metadata.log"
case "$platform" in
  macos)
    run_logged "$signing_log" \
      "$repo_root/scripts/sign-macos.sh" \
      --package "$package_path" \
      --dry-run
    signing_reference="$(resolve_logged_path "$(kv_value reference "$signing_log")")"
    signing_manifest="$(resolve_logged_path "$(kv_value manifest "$signing_log")")"
    signing_verdict="$(kv_value verdict "$signing_log")"
    ;;
  linux)
    signing_reference="target/release-reference/artifacts/signing-metadata-linux-$short_sha"
    run_logged "$signing_log" \
      "$repo_root/scripts/metadata-linux.sh" \
      --package "$package_path" \
      --output-dir "$signing_reference"
    signing_manifest=""
    signing_verdict="$(kv_value verdict "$signing_log")"
    ;;
esac

signing_reference_display="$(display_path "$signing_reference")"
signing_manifest_display=""
if [ -n "$signing_manifest" ]; then
  signing_manifest_display="$(display_path "$signing_manifest")"
fi

mkdir -p "$reference_root/logs"
cp "$tmp_root/logs/"*.log "$reference_root/logs/"

cat > "$reference_root/summary.json" <<EOF
{
  "schema": 1,
  "platform": "$platform",
  "pipeline": "release-local-v1",
  "git_commit": "$head_sha",
  "configuration": "$configuration",
  "package_path": "$(printf '%s' "$(display_path "$package_path")" | json_escape)",
  "package_sha256": "$package_sha",
  "package_checksums": "$dist_dir/SHA256SUMS.txt",
  "signing_verdict": "$signing_verdict",
  "signing_reference": "$(printf '%s' "$signing_reference_display" | json_escape)",
  "signing_manifest": "$(printf '%s' "$signing_manifest_display" | json_escape)"
}
EOF

write_checksums "$reference_root" "$reference_root/SHA256SUMS.txt"
bundle_sha="$(sha256_file "$reference_root/SHA256SUMS.txt")"

cat > "$manifest_path" <<EOF
# RMUX P3 Local Release Automation

## Verdict

PASS

## Scope

This local-first wrapper orchestrates P1 packaging, package verification, and P2
signing/metadata dry-run for $platform. It does not sign real artifacts,
publish releases, create tags, or contact CI.

## Inputs

| Item | Value |
| --- | --- |
| HEAD | \`$head_sha\` |
| Platform | \`$platform\` |
| Configuration | \`$configuration\` |
| Package | \`$(display_path "$package_path")\` |
| Package SHA256 | \`$package_sha\` |
| Signing verdict | \`$signing_verdict\` |

## reference

| Artifact | Path |
| --- | --- |
| Bundle | \`$reference_root\` |
| SHA256SUMS | \`$reference_root/SHA256SUMS.txt\` |
| SHA256SUMS SHA256 | \`$bundle_sha\` |
| Summary | \`$reference_root/summary.json\` |
| Signing reference | \`$signing_reference_display\` |
| Signing manifest | \`${signing_manifest_display:-n/a}\` |

## Commands

- \`scripts/package-unix.sh --configuration $configuration --output-dir $dist_dir\`
- \`scripts/verify-package.sh <package> --checksums $dist_dir/SHA256SUMS.txt --run-binary\`
- Platform signing/metadata dry-run via the P2B script.
EOF

printf 'verdict=PASS\n'
printf 'platform=%s\n' "$platform"
printf 'package=%s\n' "$(display_path "$package_path")"
printf 'package_sha256=%s\n' "$package_sha"
printf 'signing_verdict=%s\n' "$signing_verdict"
printf 'reference=%s\n' "$reference_root"
printf 'manifest=%s\n' "$manifest_path"
