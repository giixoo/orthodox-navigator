#!/bin/sh
# Installs Orthodox Navigator on Linux or macOS from the latest GitHub release, for the current user (no sudo).
#
#     curl -fsSL https://raw.githubusercontent.com/giixoo/orthodox-navigator/main/install.sh | sh
#
# Run it again to update. To remove:
#     curl -fsSL https://raw.githubusercontent.com/giixoo/orthodox-navigator/main/install.sh | sh -s -- --uninstall
#
# It puts the program in ~/.local/share/orthodox-navigator and adds the line that loads the `on` command to
# ~/.zshrc or ~/.bashrc (~/.bash_profile on macOS), whichever belongs to your shell.
# Settings: ON_VERSION=0.4.0 installs that version; ON_INSTALL_DIR another folder; ON_NO_PROFILE=1 leaves the
# shell's startup file alone. Put them after the pipe: ... | ON_VERSION=0.4.0 sh

set -eu

repo=giixoo/orthodox-navigator
dir=${ON_INSTALL_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/orthodox-navigator}
line="source \"$dir/shell/on.sh\""

say() { printf '%s\n' "$*"; }
die() { say "error: $*" >&2; exit 1; }

fetch() { # url [file]
    if command -v curl >/dev/null 2>&1; then
        if [ $# -gt 1 ]; then curl -fsSL -o "$2" "$1"; else curl -fsSL "$1"; fi
    elif command -v wget >/dev/null 2>&1; then
        if [ $# -gt 1 ]; then wget -qO "$2" "$1"; else wget -qO- "$1"; fi
    else
        die "needs curl or wget"
    fi
}

os=$(uname -s)
case $os in
    Linux)
        case $(uname -m) in
            x86_64 | amd64) target=linux-x64 ;;
            aarch64 | arm64) target=linux-arm64 ;;
            *) die "no build for $(uname -m); build from source (see the README)" ;;
        esac ;;
    Darwin) target=macos-universal ;;
    *) die "$os is not supported here; on Windows use install.ps1 (see the README)" ;;
esac

# the startup file of the user's shell
case $(basename "${SHELL:-sh}") in
    zsh) rc=$HOME/.zshrc ;;
    bash) if [ "$os" = Darwin ]; then rc=$HOME/.bash_profile; else rc=$HOME/.bashrc; fi ;;
    *) rc= ;;
esac

if [ "${1-}" = --uninstall ]; then
    for f in "$HOME/.zshrc" "$HOME/.bashrc" "$HOME/.bash_profile"; do
        if [ -f "$f" ] && grep -qxF "$line" "$f"; then
            grep -vxF -e "$line" -e '# Orthodox Navigator' "$f" > "$f.on-tmp" || true
            cat "$f.on-tmp" > "$f" && rm -f "$f.on-tmp"   # cat keeps the file's permissions and links
        fi
    done
    rm -rf "$dir"
    say "Orthodox Navigator removed from $dir. Open a new terminal."
    exit 0
fi

if [ -n "${ON_VERSION-}" ]; then
    v=${ON_VERSION#v}
else
    v=$(fetch "https://api.github.com/repos/$repo/releases/latest" |
        sed -n 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/p' | head -n 1)
    [ -n "$v" ] || die "couldn't find the latest release (GitHub API limit?); set ON_VERSION"
fi
name=on-$v-$target
say "Orthodox Navigator $v -> $dir"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fetch "https://github.com/$repo/releases/download/v$v/$name.tar.gz" "$tmp/on.tar.gz"
tar -xzf "$tmp/on.tar.gz" -C "$tmp"

mkdir -p "$dir/bin" "$dir/shell"
# copy next to it, then rename: a running `on` can't be overwritten ("text file busy"), but it can be replaced
cp "$tmp/$name/bin/on" "$dir/bin/on.new"
chmod +x "$dir/bin/on.new"
mv -f "$dir/bin/on.new" "$dir/bin/on"
cp "$tmp/$name/shell/on.sh" "$dir/shell/on.sh"
cp "$tmp/$name/README.md" "$tmp/$name/LICENSE" "$dir/"

if [ "${ON_NO_PROFILE-}" = 1 ] || [ -z "$rc" ]; then
    say "Done. To get the on command, add this line to your shell's startup file:"
    say "    $line"
elif [ -f "$rc" ] && grep -q 'on\.sh' "$rc"; then
    say "Done ($rc already loads on.sh). Open a new terminal and type: on"
else
    printf '\n# Orthodox Navigator\n%s\n' "$line" >> "$rc"
    say "Done. Open a new terminal and type: on"
fi
