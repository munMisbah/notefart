#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Pemakaian: install.sh [--prefix ABS] [--replace] [--skip-config]
                  [--theme dark|light|terminal] [--color auto|truecolor|256|16|none]
                  [--icons unicode|nerd|ascii] [--clipboard auto|internal|system]
                  [--help]
Memasang naf dan metadata dari bundle lokal. Konfigurasi lama tidak ditimpa.
EOF
}
fail() { printf 'Error: %s\n' "$1" >&2; exit 1; }
show_path() { printf '%q' "$1"; }

prefix= explicit_prefix=0 replace=0 skip_config=0
theme=dark color=auto icons=unicode clipboard=auto
while (($#)); do
    case $1 in
        --help) usage; exit 0 ;;
        --replace) replace=1; shift ;;
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
if (( EUID == 0 )) && (( ! explicit_prefix || ! skip_config )); then
    fail 'Root wajib menggunakan --prefix eksplisit dan --skip-config'
fi
if (( ! explicit_prefix )); then
    [[ ${HOME-} == /* ]] || fail 'HOME absolut diperlukan untuk prefix default'
    prefix=${HOME%/}/.local
fi
[[ $prefix == /* ]] || fail 'Prefix harus absolut'

config=
if (( ! skip_config )); then
    if [[ ${XDG_CONFIG_HOME-} == /* ]]; then
        config=${XDG_CONFIG_HOME%/}/notefart/config.toml
    elif [[ ${HOME-} == /* ]]; then
        config=${HOME%/}/.config/notefart/config.toml
    else
        fail 'XDG_CONFIG_HOME atau HOME absolut diperlukan untuk konfigurasi'
    fi
fi

bundle=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P) || fail 'Direktori bundle tidak dapat dibaca'
sources=("$bundle/naf" "$bundle/VERSION" "$bundle/config.toml.example" "$bundle/licenses/Oniguruma.txt" "$bundle/licenses/Sublime-Packages.txt")
targets=("$prefix/bin/naf" "$prefix/share/notefart/VERSION" "$prefix/share/notefart/config.toml.example" "$prefix/share/notefart/licenses/Oniguruma.txt" "$prefix/share/notefart/licenses/Sublime-Packages.txt")
for source in "${sources[@]}"; do
    [[ ! -L $source && -f $source && -r $source ]] || fail 'Payload bundle tidak lengkap atau bukan file reguler'
done
[[ -x ${sources[0]} ]] || fail 'Binary bundle tidak dapat dieksekusi'

check_target() {
    local path=$1
    [[ ! -L $path ]] || fail "Target symlink ditolak: $(show_path "$path")"
    if [[ -e $path && ! -f $path ]]; then
        fail "Target bukan file reguler: $(show_path "$path")"
    fi
}
# Periksa semua target sebelum publikasi apa pun.
for i in "${!targets[@]}"; do
    check_target "${targets[i]}"
    if [[ -e ${targets[i]} ]] && (( ! replace )); then
        cmp -s -- "${sources[i]}" "${targets[i]}" || fail "Target berbeda; gunakan --replace: $(show_path "${targets[i]}")"
    fi
done
if [[ -n $config ]]; then check_target "$config"; fi

mkdir -p -- "$prefix/bin" "$prefix/share/notefart/licenses" 2>/dev/null || fail 'Direktori instalasi tidak dapat dibuat'
if [[ -n $config && ! -e $config ]]; then
    mkdir -p -- "${config%/*}" 2>/dev/null || fail 'Direktori konfigurasi tidak dapat dibuat'
fi

stages=()
cleanup() {
    local dir
    for dir in "${stages[@]}"; do rm -rf -- "$dir"; done
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
new_stage() {
    STAGE=$(mktemp -d -- "$1/.naf-install.XXXXXXXX" 2>/dev/null) || fail 'Direktori staging tidak dapat dibuat'
    stages+=("$STAGE")
}

# Semua salinan disiapkan pada filesystem direktori tujuan sebelum publikasi.
staged=()
for i in "${!targets[@]}"; do
    new_stage "${targets[i]%/*}"
    staged[i]=$STAGE/file
    cp -- "${sources[i]}" "${staged[i]}" 2>/dev/null || fail 'Menyiapkan payload gagal'
    if (( i == 0 )); then mode=755; else mode=644; fi
    chmod "$mode" -- "${staged[i]}" 2>/dev/null || fail 'Mengatur mode payload gagal'
done
config_stage=
if [[ -n $config && ! -e $config ]]; then
    new_stage "${config%/*}"
    config_stage=$STAGE/config.toml
    if ! "${sources[0]}" --config "$config_stage" --save-config --theme "$theme" --color "$color" --icons "$icons" --clipboard "$clipboard" >/dev/null; then
        fail 'Binary bundle gagal menghasilkan konfigurasi'
    fi
    [[ ! -L $config_stage && -f $config_stage ]] || fail 'Konfigurasi staging bukan file reguler'
    chmod 600 -- "$config_stage" 2>/dev/null || fail 'Mengatur mode konfigurasi gagal'
fi

for i in "${!targets[@]}"; do
    target=${targets[i]}
    check_target "$target"
    if [[ -e $target ]] && (( ! replace )); then
        cmp -s -- "${sources[i]}" "$target" || fail "Target berubah; gunakan --replace: $(show_path "$target")"
        continue
    fi
    if (( replace )); then
        mv -fT -- "${staged[i]}" "$target" 2>/dev/null || fail "Publikasi gagal: $(show_path "$target")"
    else
        ln -T -- "${staged[i]}" "$target" 2>/dev/null || fail "Publikasi no-clobber gagal: $(show_path "$target")"
    fi
done
if [[ -n $config ]]; then
    check_target "$config"
    if [[ -e $config ]]; then
        printf 'Konfigurasi lama dipertahankan; preferensi yang diminta tidak diterapkan: %q\n' "$config"
    elif [[ -n $config_stage ]]; then
        if ! ln -T -- "$config_stage" "$config" 2>/dev/null; then
            check_target "$config"
            if [[ -e $config ]]; then
                printf 'Konfigurasi lain telah dibuat; preferensi yang diminta tidak diterapkan: %q\n' "$config"
            else
                fail "Publikasi konfigurasi gagal: $(show_path "$config")"
            fi
        else
            printf 'Konfigurasi dibuat: %q\n' "$config"
        fi
    else
        fail 'Konfigurasi berubah selama instalasi; jalankan kembali installer'
    fi
else
    printf 'Konfigurasi dilewati; jalankan naf --save-config sebagai pengguna tujuan untuk setup preferensi.\n'
fi
printf 'Binary terpasang: %q\n' "${targets[0]}"
printf 'Pastikan direktori berikut ada dalam PATH (installer tidak mengubah PATH): %q\n' "$prefix/bin"
