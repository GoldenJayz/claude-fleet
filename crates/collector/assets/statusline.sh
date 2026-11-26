#!/usr/bin/env bash
# Pane status line.
# Claude Code pipes its status JSON (including plan rate limits) to this script.
# We save it next to this script for the Pane collector, then print a status line.
# Nothing leaves this machine.

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
input="$(cat)"

# Keep the last capture that actually carried rate limits (early in a session they are absent).
case "$input" in
  *'"rate_limits"'*)
    tmp="$DIR/statusline.json.$$.tmp"
    printf '%s' "$input" > "$tmp" && mv -f "$tmp" "$DIR/statusline.json"
    ;;
esac

# If you already had a status line, Pane runs it unchanged.
if [ -s "$DIR/statusline-chain.sh" ]; then
  printf '%s' "$input" | bash "$DIR/statusline-chain.sh"
  exit $?
fi

pick() { printf '%s' "$input" | sed -n "s/.*$1[[:space:]]*:[[:space:]]*{[[:space:]]*\"used_percentage\"[[:space:]]*:[[:space:]]*\([0-9.]*\).*/\1/p" | head -n 1; }
model=$(printf '%s' "$input" | sed -n 's/.*"display_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)
five=$(pick '"five_hour"')
week=$(pick '"seven_day"')

line="${model:-Claude}"
[ -n "$five" ] && line="$line · 5h ${five%%.*}%"
[ -n "$week" ] && line="$line · week ${week%%.*}%"
printf '%s' "$line"
