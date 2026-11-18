# Orthodox Navigator shell integration for bash and zsh (Linux / macOS).
#
# Add this line to ~/.bashrc or ~/.zshrc:
#     source /path/to/orthodox-navigator/shell/on.sh
# (installed from a .deb / .rpm package: source /usr/share/orthodox-navigator/on.sh)
#
# Then type `on`. Commands you type in the file manager run right here, in this shell session -
# your aliases, functions and variables work, and a `cd` is followed. The file manager steps aside
# while a command runs and comes back exactly as it was (Ctrl+O shows the output).
# When you quit (Alt+X) you stay in the folder of the active panel, and every command you ran is
# in your history.
#
# Set ORTHODOX_NAVIGATOR_EXE to use a binary somewhere else.

if [ -n "${ZSH_VERSION-}" ]; then
    _on_dir=${${(%):-%x}:A:h}
else
    _on_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
fi
if [ -z "${ORTHODOX_NAVIGATOR_EXE-}" ]; then
    if [ -x "$_on_dir/../bin/on" ]; then ORTHODOX_NAVIGATOR_EXE=$_on_dir/../bin/on
    elif [ -x "$_on_dir/../../bin/on" ]; then ORTHODOX_NAVIGATOR_EXE=$_on_dir/../../bin/on   # /usr/share/orthodox-navigator -> /usr/bin
    else ORTHODOX_NAVIGATOR_EXE=$_on_dir/../rust/target/release/on; fi
fi
unset _on_dir

on() {
    local __on_cwd __on_hist __on_sess __on_act __on_line __on_text __on_dir
    __on_cwd=$(mktemp) && __on_hist=$(mktemp) && __on_sess=$(mktemp) && __on_act=$(mktemp) || return 1

    while :; do
        : > "$__on_act"
        "$ORTHODOX_NAVIGATOR_EXE" --cwd-file "$__on_cwd" --history-file "$__on_hist" \
            --session-file "$__on_sess" --action-file "$__on_act" --shell "${SHELL:-/bin/sh}" "$@"

        while IFS= read -r __on_line || [ -n "$__on_line" ]; do
            [ -z "$__on_line" ] && continue
            if [ -n "${ZSH_VERSION-}" ]; then print -s -- "$__on_line"; else history -s -- "$__on_line"; fi
        done < "$__on_hist"
        : > "$__on_hist"

        __on_dir=$(cat -- "$__on_cwd")
        [ -n "$__on_dir" ] && [ -d "$__on_dir" ] && cd -- "$__on_dir"

        __on_text=$(cat -- "$__on_act")
        case "$__on_text" in
            run$'\n'*)
                # run the command here, in this session, as if typed at the prompt
                printf '%s$ %s\n' "$PWD" "${__on_text#run$'\n'}"
                eval "${__on_text#run$'\n'}"
                echo
                ;;
            *) break ;;
        esac
    done
    rm -f -- "$__on_cwd" "$__on_hist" "$__on_sess" "$__on_act"
}
