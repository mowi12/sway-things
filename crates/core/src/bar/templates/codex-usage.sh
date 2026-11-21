#!/bin/sh
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
# Sway Things template: a waybar custom module. It is yours once copied; waybar runs it.
#
# UNOFFICIAL: how much of the Codex usage limits of a ChatGPT plan is used, from the endpoint
# the Codex usage page reads. It uses the Codex CLI's own login and an undocumented endpoint
# that may change or stop working at any time.
#
# The login is read where the Codex CLI keeps it (~/.codex/auth.json) and only sent to OpenAI's
# endpoint, as headers on curl's standard input, never on a command line. It is never copied,
# stored or written; when it has expired, run codex once to renew it. Only the answer
# (percentages, no login) is kept for two minutes in $XDG_CACHE_HOME. Needs curl and jq.
#
#   codex-usage.sh          JSON for waybar
#   codex-usage.sh --force  ask again now (for a click)
set -u

auth=${CODEX_AUTH:-${CODEX_HOME:-$HOME/.codex}/auth.json}
cache=${XDG_CACHE_HOME:-$HOME/.cache}/sway-things-codex-usage.json
endpoint=https://chatgpt.com/backend-api/wham/usage

for program in curl jq; do
  if ! command -v "$program" >/dev/null; then
    printf '{"text": "", "tooltip": "Codex usage needs %s", "class": "down"}\n' "$program"
    exit 0
  fi
done
if [ ! -r "$auth" ]; then
  printf '{"text": "", "tooltip": "Not logged in to Codex", "class": "down"}\n'
  exit 0
fi
# Logged in with an API key instead of a ChatGPT plan: there are no plan limits to show.
if [ -z "$(jq -r '.tokens.access_token // empty' "$auth" 2>/dev/null)" ]; then
  printf '{"text": "", "tooltip": "Codex uses an API key, which has no plan limits to show", "class": "down"}\n'
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
elif [ "${1:-}" = --force ] || [ "$age" -ge 120 ]; then
  answer=$(jq -r '"Authorization: Bearer " + (.tokens.access_token // ""),
      "ChatGPT-Account-Id: " + (.tokens.account_id // "")' "$auth" |
    curl -q -sS --max-time 10 -w '\n%{http_code}' -H @- -H 'Accept: application/json' \
      "$endpoint" 2>/dev/null)
  code=$(printf '%s' "$answer" | tail -n 1)
  body=$(printf '%s' "$answer" | sed '$d')
  case $code in
    200)
      printf '%s' "$body" | jq -c '{rate_limit}' >"$cache.tmp" 2>/dev/null &&
        mv "$cache.tmp" "$cache"
      rm -f "$cache.retry"
      ;;
    401 | 403)
      problem="The login expired: run codex once"
      echo $((now + 60)) >"$cache.retry"
      ;;
    429)
      problem="OpenAI asks to slow down; trying again in ten minutes"
      echo $((now + 600)) >"$cache.retry"
      ;;
    *)
      problem="No answer from OpenAI (HTTP $code)"
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
  (.rate_limit.primary_window.used_percent | pct) as $five
  | (.rate_limit.secondary_window.used_percent | pct) as $week
  | ([$five, $week] | max) as $worst
  | {
      text: "Codex \($five) %",
      tooltip: ("Codex, unofficial\n5 hours: \($five) %\nWeek: \($week) %"
        + (if $problem != "" then "\n" + $problem else "" end)),
      class: (if $worst >= 100 then "critical" elif $worst >= 80 then "warning" else "normal" end),
      percentage: $worst
    }' "$cache"
