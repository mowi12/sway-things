#!/bin/sh
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
# Sway Things template: a waybar custom module. It is yours once copied; waybar runs it.
#
# UNOFFICIAL: how much of Claude Code's usage limits is used, from the endpoint the Claude
# website's usage page reads. It uses Claude Code's own login and an undocumented endpoint
# that may change or stop working at any time.
#
# The login is read where Claude Code keeps it (~/.claude/.credentials.json) and only sent to
# Anthropic's endpoint, as a header on curl's standard input, never on a command line. It is
# never copied, stored or written; when it has expired, run claude once to renew it. Only the
# answer (percentages, no login) is kept for five minutes in $XDG_CACHE_HOME. Needs curl and jq.
#
#   claude-usage.sh          JSON for waybar
#   claude-usage.sh --force  ask again now (for a click)
set -u

credentials=${CLAUDE_CREDENTIALS:-$HOME/.claude/.credentials.json}
cache=${XDG_CACHE_HOME:-$HOME/.cache}/sway-things-claude-usage.json
endpoint=https://api.anthropic.com/api/oauth/usage

for program in curl jq; do
  if ! command -v "$program" >/dev/null; then
    printf '{"text": "", "tooltip": "Claude usage needs %s", "class": "down"}\n' "$program"
    exit 0
  fi
done
if [ ! -r "$credentials" ]; then
  printf '{"text": "", "tooltip": "Not logged in to Claude Code", "class": "down"}\n'
  exit 0
fi

# The cache holds only the answer's percentages; nobody else may read it.
umask 077
mkdir -p "${cache%/*}"
now=$(date +%s)
age=999999
[ -f "$cache" ] && age=$((now - $(stat -c %Y "$cache")))
# After a failure, wait before asking again: a minute, or ten after "too many requests".
retry=0
[ -f "$cache.retry" ] && read -r retry <"$cache.retry"
case $retry in '' | *[!0-9]*) retry=0 ;; esac
problem=""
if [ "$now" -lt "$retry" ]; then
  problem="Asked too often; trying again later"
elif [ "${1:-}" = --force ] || [ "$age" -ge 300 ]; then
  answer=$(jq -r '"Authorization: Bearer " + (.claudeAiOauth.accessToken // "")' "$credentials" |
    curl -q -sS --max-time 10 -w '\n%{http_code}' -H @- \
      -H 'anthropic-beta: oauth-2025-04-20' -H 'Accept: application/json' "$endpoint" 2>/dev/null)
  code=$(printf '%s' "$answer" | tail -n 1)
  body=$(printf '%s' "$answer" | sed '$d')
  case $code in
    200)
      printf '%s' "$body" | jq -c '{five_hour, seven_day}' >"$cache.tmp" 2>/dev/null &&
        mv "$cache.tmp" "$cache"
      rm -f "$cache.retry"
      ;;
    401 | 403)
      problem="The login expired: run claude once"
      echo $((now + 60)) >"$cache.retry"
      ;;
    429)
      problem="Anthropic asks to slow down; trying again in ten minutes"
      echo $((now + 600)) >"$cache.retry"
      ;;
    *)
      problem="No answer from Anthropic (HTTP $code)"
      echo $((now + 60)) >"$cache.retry"
      ;;
  esac
fi
if [ ! -s "$cache" ]; then
  printf '{"text": "", "tooltip": "%s", "class": "down"}\n' "${problem:-No usage yet}"
  exit 0
fi
jq -c --arg problem "$problem" '
  def pct: (. // 0 | floor);
  (.five_hour.utilization | pct) as $five
  | (.seven_day.utilization | pct) as $week
  | ([$five, $week] | max) as $worst
  | {
      text: "AI \($five) %",
      tooltip: ("Claude, unofficial\n5 hours: \($five) %\nWeek: \($week) %"
        + (if $problem != "" then "\n" + $problem else "" end)),
      class: (if $worst >= 100 then "critical" elif $worst >= 80 then "warning" else "normal" end),
      percentage: $worst
    }' "$cache"
