#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Pemakaian: install.sh [--prefix ABS] [--replace] [--skip-config]
                  [--theme dark|light|terminal] [--color auto|truecolor|256|16|none]
                  [--icons unicode|nerd|ascii] [--clipboard auto|internal|system]
                  [--help]
Mengunduh bundle latest Release dan menjalankan installer bundle.
Konfigurasi lama tidak ditimpa.
EOF
}
fail() { printf 'Error: %s\n' "$1" >&2; exit 1; }

args=("$@")
prefix= explicit_prefix=0 skip_config=0
theme=dark color=auto icons=unicode clipboard=auto
while (($#)); do
    case $1 in
        --help) usage; exit 0 ;;
        --replace) shift ;;
        --skip-config) skip_config=1; shift ;;
        --prefix|--theme|--color|--icons|--clipboard)
            option=$1
            (($# >= 2)) || fail "Nilai $option diperlukan"
            case $option in
                --prefix) prefix=$2; explicit_prefix=1 ;;
                --theme) theme=$2 ;;
                --color) color=$2 ;;
                --icons) icons=$2 ;;
                --clipboard) clipboard=$2 ;;
            esac
            shift 2 ;;
        *) fail 'Opsi tidak dikenal; gunakan --help' ;;
    esac
done
case $theme in dark|light|terminal) ;; *) fail 'Nilai --theme tidak valid' ;; esac
case $color in auto|truecolor|256|16|none) ;; *) fail 'Nilai --color tidak valid' ;; esac
case $icons in unicode|nerd|ascii) ;; *) fail 'Nilai --icons tidak valid' ;; esac
case $clipboard in auto|internal|system) ;; *) fail 'Nilai --clipboard tidak valid' ;; esac
if (( explicit_prefix )); then [[ $prefix == /* ]] || fail 'Prefix harus absolut'; fi

for command in uname curl tar sha256sum mktemp mkdir chmod rm; do
    command -v "$command" >/dev/null 2>&1 || fail "Perintah diperlukan: $command"
done
[[ $(uname -s) == Linux && $(uname -m) == x86_64 ]] || fail 'Hanya GNU/Linux x86_64 yang didukung'
if (( EUID == 0 )) && (( ! explicit_prefix || ! skip_config )); then
    fail 'Root wajib menggunakan --prefix eksplisit dan --skip-config'
fi

umask 077
stage=$(mktemp -d) || fail 'Direktori sementara tidak dapat dibuat'
cleanup() { rm -rf -- "$stage"; }
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
archive=naf-linux-x86_64.tar.gz
base=https://github.com/munMisbah/notefart/releases/latest/download
curl --proto '=https' --proto-redir '=https' -fsSL "$base/$archive" -o "$stage/$archive" || fail 'Download archive gagal'
curl --proto '=https' --proto-redir '=https' -fsSL "$base/$archive.sha256" -o "$stage/$archive.sha256" || fail 'Download checksum gagal'
checksum=$(<"$stage/$archive.sha256")
[[ $checksum =~ ^([[:xdigit:]]{64})[[:space:]][\ \*]naf-linux-x86_64\.tar\.gz$ ]] || fail 'Format checksum tidak valid'
(cd -- "$stage" && printf '%s  %s\n' "${BASH_REMATCH[1]}" "$archive" | sha256sum -c --status) || fail 'Checksum archive tidak cocok'

# Periksa nama dan jenis setiap entri sebelum membaca payload; tidak ada path
# atau metadata archive yang diterapkan ke filesystem.
tar -tzf "$stage/$archive" --quoting-style=escape > "$stage/names" || fail 'Archive tidak dapat dibaca'
tar -tvzf "$stage/$archive" --quoting-style=escape > "$stage/types" || fail 'Metadata archive tidak dapat dibaca'
mapfile -t names < "$stage/names"
mapfile -t types < "$stage/types"
((${#names[@]} == ${#types[@]})) || fail 'Metadata archive tidak konsisten'
files=(naf install.sh VERSION config.toml.example licenses/Oniguruma.txt licenses/Sublime-Packages.txt)
declare -A seen=()
for i in "${!names[@]}"; do
    name=${names[i]}
    [[ ! ${seen[$name]+present} ]] || fail 'Entri archive duplikat'
    seen[$name]=1
    case $name in
        naf-linux-x86_64/|naf-linux-x86_64/licenses/)
            [[ ${types[i]:0:1} == d ]] || fail 'Direktori archive tidak valid' ;;
        naf-linux-x86_64/naf|naf-linux-x86_64/install.sh|naf-linux-x86_64/VERSION|naf-linux-x86_64/config.toml.example|naf-linux-x86_64/licenses/Oniguruma.txt|naf-linux-x86_64/licenses/Sublime-Packages.txt)
            [[ ${types[i]:0:1} == - ]] || fail 'Payload archive bukan file reguler' ;;
        *) fail 'Entri archive tidak diharapkan' ;;
    esac
done
bundle=$stage/bundle
mkdir -p -- "$bundle/licenses" || fail 'Stage payload tidak dapat dibuat'
for file in "${files[@]}"; do
    name=naf-linux-x86_64/$file
    [[ ${seen[$name]+present} ]] || fail 'Payload archive tidak lengkap'
    tar -xOzf "$stage/$archive" -- "$name" > "$bundle/$file" || fail 'Ekstraksi payload gagal'
    [[ -s $bundle/$file ]] || fail 'Payload archive kosong'
done
chmod 755 -- "$bundle/naf" || fail 'Binary tidak dapat disiapkan'
status=0
bash "$bundle/install.sh" "${args[@]}" || status=$?
exit "$status"
