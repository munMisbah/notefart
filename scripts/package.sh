#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo 'Usage: package.sh --binary PATH --output DIR' >&2
  exit 2
}

binary=
output=
while (($#)); do
  case "$1" in
    --binary) (($# >= 2)) || usage; binary=$2; shift 2 ;;
    --output) (($# >= 2)) || usage; output=$2; shift 2 ;;
    *) usage ;;
  esac
done
[[ -n $binary && -n $output ]] || usage

repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
semver='^naf ((0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-((0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)(\.(0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*))*))?(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?)$'
version_output=$("$binary" --version)
if [[ ! $version_output =~ $semver ]]; then
  echo "Invalid naf version output" >&2
  exit 1
fi
version=${BASH_REMATCH[1]}

mkdir -p -- "$output"
stage=$(mktemp -d -- "$output/.naf-package.XXXXXXXX")
trap 'rm -rf -- "$stage"' EXIT
bundle=$stage/naf-linux-x86_64
mkdir -p -- "$bundle/licenses"
install -m 755 -- "$binary" "$bundle/naf"
install -m 755 -- "$repo/scripts/install.sh" "$bundle/install.sh"
printf '%s\n' "$version" > "$bundle/VERSION"
chmod 644 "$bundle/VERSION"
install -m 644 -- "$repo/config.toml.example" "$bundle/config.toml.example"
install -m 644 -- "$repo/assets/licenses/Oniguruma.txt" "$bundle/licenses/Oniguruma.txt"
install -m 644 -- "$repo/assets/syntaxes/LICENSE" "$bundle/licenses/Sublime-Packages.txt"
tar -czf "$stage/naf-linux-x86_64.tar.gz" -C "$stage" naf-linux-x86_64
(cd "$stage" && sha256sum naf-linux-x86_64.tar.gz > naf-linux-x86_64.tar.gz.sha256)
# Do not leave an old checksum claiming to describe a newly replaced archive.
rm -f -- "$output/naf-linux-x86_64.tar.gz.sha256"
mv -f -- "$stage/naf-linux-x86_64.tar.gz" "$output/naf-linux-x86_64.tar.gz"
mv -f -- "$stage/naf-linux-x86_64.tar.gz.sha256" "$output/naf-linux-x86_64.tar.gz.sha256"
printf 'Paket dan checksum tersedia di %q\n' "$output"
