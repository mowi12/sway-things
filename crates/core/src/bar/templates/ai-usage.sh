#!/bin/sh
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-FileCopyrightText: 2026 nurkert
# SPDX-License-Identifier: GPL-3.0-or-later
# Sway Things template: a waybar custom module. It is yours once copied; waybar runs it.
#
# UNOFFICIAL: how much of the usage limits of command line agents is used: Claude Code (5 hours,
# the week, the weekly limits of single models) and Codex on a ChatGPT plan (5 hours, the week).
# Each asks the endpoint the vendor's own usage page reads (api.anthropic.com, chatgpt.com), with
# the login that vendor's command line tool keeps. These endpoints are undocumented and may change
# or stop working at any time. This script uses the network; nothing else of Sway Things does.
#
# Logins are read where the tools keep them (~/.claude/.credentials.json, ~/.codex/auth.json) and
# only sent to that vendor's endpoint, as headers on curl's standard input, never on a command
# line. They are never copied, stored, written or renewed: when one has expired, run the tool once
# and it renews its own. The script sends curl's own User-Agent and starts no other program in the
# background. Only the answers (percentages and reset times, no login) are kept in
# $XDG_CACHE_HOME/sway-things/ai-usage, shared by every module of the pill, so the pill asks each
# vendor at most every five minutes. Needs curl and jq.
#
#   ai-usage.sh show AGENT             one agent, always shown
#   ai-usage.sh head AGENT...          the first agent that is logged in, with its more
#                                      critical window
#   ai-usage.sh alert AGENT HEAD...    AGENT only at 80 % and more or at its limit, unless it is
#                                      the head of the agents HEAD...
#   ai-usage.sh detail AGENT HEAD...   AGENT in detail, without what the head or alert shows
#   ai-usage.sh refresh AGENT          ask again now, then redraw the pill (a click)
#   ai-usage.sh open AGENT             the vendor's usage page in the browser (a right click)
#
# AGENT is claude or codex. A picture of the agent replaces its name where
# ~/.config/sway-things/waybar/icons/AGENT.svg exists (a folder of your own can be linked there).
set -u

signal=9
cache_dir=${XDG_CACHE_HOME:-$HOME/.cache}/sway-things/ai-usage
icons=${XDG_CONFIG_HOME:-$HOME/.config}/sway-things/waybar/icons
claude_login=${CLAUDE_CREDENTIALS:-$HOME/.claude/.credentials.json}
codex_login=${CODEX_AUTH:-${CODEX_HOME:-$HOME/.codex}/auth.json}

empty() {
  printf '{"text": "", "tooltip": "%s", "class": "idle"}\n' "${1:-}"
  exit 0
}

for program in curl jq; do
  command -v "$program" >/dev/null || empty "The usage of agents needs $program"
done

mode=${1:-show}
agent=${2:-claude}
if [ $# -ge 2 ]; then shift 2; else shift $#; fi
case $agent in claude | codex) ;; *) empty "Unknown agent: $agent" ;; esac

# Whether `agent` has a login to ask with.
logged_in() {
  case $1 in
    claude) [ -r "$claude_login" ] ;;
    codex) [ -r "$codex_login" ] && [ -n "$(jq -r '.tokens.access_token // empty' "$codex_login" 2>/dev/null)" ] ;;
    *) false ;;
  esac
}

# The head of the agents given: the first that is logged in.
head_of() {
  for candidate in "$@"; do
    if logged_in "$candidate"; then
      printf '%s' "$candidate"
      return
    fi
  done
}

# Asks the vendor of `agent` and keeps the answer, unless it is fresh (or `force`).
fetch() {
  name=$1 force=$2
  umask 077
  mkdir -p "$cache_dir"
  cache=$cache_dir/$name.json
  now=$(date +%s)
  age=999999
  [ -f "$cache" ] && age=$((now - $(stat -c %Y "$cache")))
  [ "$force" = 1 ] || [ "$age" -ge 300 ] || return 0
  # Every module of the pill runs this, on every display: one asks, the others wait and find
  # the fresh answer.
  if command -v flock >/dev/null; then
    exec 9>"$cache.lock"
    flock -w 20 9 || true
    [ -f "$cache" ] && age=$(($(date +%s) - $(stat -c %Y "$cache")))
    [ "$force" = 1 ] || [ "$age" -ge 300 ] || return 0
  fi
  # After a failure, wait before asking again: a minute, or ten after "too many requests".
  retry=0
  [ -f "$cache.retry" ] && read -r retry <"$cache.retry"
  case $retry in '' | *[!0-9]*) retry=0 ;; esac
  if [ "$force" != 1 ] && [ "$now" -lt "$retry" ]; then
    return 0
  fi
  case $name in
    claude)
      answer=$(jq -r '"Authorization: Bearer " + (.claudeAiOauth.accessToken // "")' "$claude_login" |
        curl -q -sS --max-time 10 -w '\n%{http_code}' -H @- \
          -H 'anthropic-beta: oauth-2025-04-20' -H 'Accept: application/json' \
          https://api.anthropic.com/api/oauth/usage 2>/dev/null)
      keep='{five_hour, seven_day, seven_day_opus, seven_day_sonnet, limits}'
      ;;
    codex)
      answer=$(jq -r '"Authorization: Bearer " + (.tokens.access_token // ""),
          "ChatGPT-Account-Id: " + (.tokens.account_id // "")' "$codex_login" |
        curl -q -sS --max-time 10 -w '\n%{http_code}' -H @- -H 'Accept: application/json' \
          https://chatgpt.com/backend-api/wham/usage 2>/dev/null)
      keep='{plan_type, rate_limit}'
      ;;
  esac
  code=$(printf '%s' "$answer" | tail -n 1)
  body=$(printf '%s' "$answer" | sed '$d')
  case $code in
    200)
      if printf '%s' "$body" | jq -c "$keep" >"$cache.tmp" 2>/dev/null; then
        mv "$cache.tmp" "$cache"
        rm -f "$cache.retry" "$cache.problem"
      fi
      ;;
    401 | 403)
      echo "The login expired: run $name once" >"$cache.problem"
      echo $((now + 60)) >"$cache.retry"
      ;;
    429)
      echo "Asked too often; trying again in ten minutes" >"$cache.problem"
      echo $((now + 600)) >"$cache.retry"
      ;;
    *)
      echo "No answer (HTTP ${code:-none})" >"$cache.problem"
      echo $((now + 60)) >"$cache.retry"
      ;;
  esac
}

# The usage of `agent` as one JSON object: its name, plan and windows ({key, label, p, reset}),
# each window's percentage and reset (seconds since the epoch), and the weekly limits of models.
normalized() {
  name=$1
  cache=$cache_dir/$name.json
  [ -s "$cache" ] || return 1
  plan=""
  case $name in
    claude) plan=$(jq -r '.claudeAiOauth.subscriptionType // ""' "$claude_login" 2>/dev/null) ;;
  esac
  problem=""
  [ -f "$cache.problem" ] && problem=$(head -n 1 "$cache.problem")
  jq -c --arg agent "$name" --arg plan "$plan" --arg problem "$problem" \
    --argjson now "$(date +%s)" --argjson fetched "$(stat -c %Y "$cache")" '
    def epoch: if . == null then null
      elif type == "number" then .
      else (sub("\\.[0-9]+"; "") | sub("[+-]00:00$"; "Z") | try fromdateiso8601 catch null) end;
    def pct: (. // 0) | if type == "number" then . else (tonumber? // 0) end | floor;
    def window($key; $label; $p; $reset):
      ($reset | epoch) as $at
      | {key: $key, label: $label, reset: $at,
         p: (if $at != null and $at <= $now then 0 else ($p | pct) end)};
    if $agent == "claude" then
      . as $root
      | ([.limits[]? | select(.kind == "weekly_scoped" and .scope.model != null)
          | window("model"; .scope.model.display_name // "?"; .percent; .resets_at)]) as $models
      | ($models | map(.label | ascii_downcase)) as $seen
      | {name: "Claude", plan: $plan,
         windows: [window("5h"; "5 hours"; $root.five_hour.utilization; $root.five_hour.resets_at),
                   window("week"; "Week"; $root.seven_day.utilization; $root.seven_day.resets_at)],
         models: ($models + [["seven_day_opus", "Opus"], ["seven_day_sonnet", "Sonnet"]
                  | . as [$field, $label]
                  | select($root[$field].utilization? != null
                           and ($seen | index($label | ascii_downcase)) == null)
                  | window("model"; $label; $root[$field].utilization; $root[$field].resets_at)])}
    else
      def codex($w): ($w.reset_at // (if $w.reset_after_seconds != null
          then $fetched + $w.reset_after_seconds else null end));
      {name: "Codex", plan: (.plan_type // ""),
       windows: [(.rate_limit.primary_window // empty | window("5h"; "5 hours"; .used_percent; codex(.))),
                 (.rate_limit.secondary_window // empty | window("week"; "Week"; .used_percent; codex(.)))],
       models: []}
    end
    + {agent: $agent, problem: $problem,
       stale: (if $problem != "" and ($now - $fetched) > 600 then (($now - $fetched) / 60 | floor) else null end)}
  ' "$cache" 2>/dev/null
}

# What the modules show, from the usage of `agent` (normalized) and its mode.
render() {
  logo=false
  [ -f "$icons/$1.svg" ] && logo=true
  jq -c --arg mode "$2" --argjson logo "$logo" --argjson now "$(date +%s)" '
    def countdown($at):
      if $at == null then "?"
      else ($at - $now) as $left
        | if $left <= 0 then "now"
          elif $left > 18000 then ($at | strflocaltime(if $left > 72000 then "%a %H:%M" else "%H:%M" end))
          else (($left / 60) | ceil) as $m
            | if $m >= 60 then "\($m / 60 | floor)h \($m % 60)m" else "\($m)m" end
          end
      end;
    def icon: if .key == "week" then "󰃰" else "󰅐" end;
    def esc: tostring | gsub("&"; "&amp;") | gsub("<"; "&lt;") | gsub(">"; "&gt;");
    # In the pill a window shows as its symbol (a clock: hours, a calendar: the week) and its
    # percentage; in the drawer models and model families show by name.
    def short: "\(icon) \(.p)%";
    def shown: if .key == "model" then "\(.label | esc) \(.p)%" else short end;
    def class($p; $limited): if $limited then "limited" elif $p >= 80 then "crit"
      elif $p >= 50 then "warn" else "ok" end;
    . as $u
    | ($u.windows | max_by(.p) // {key: "5h", label: "5 hours", p: 0, reset: null}) as $worst
    | ([$u.windows[] | select(.p >= 100)]) as $full
    | ($full | length > 0) as $limited
    | ([$full[].reset | select(. != null)] | max) as $back
    | (if $logo then "" else "<span alpha=\"70%\" size=\"85%\">\($u.name)</span> " end) as $mark
    | (if $limited then "󰔟 \(countdown($back))"
       elif $worst.p >= 90 then "\($worst | short)  󰔟 \(countdown($worst.reset))"
       else ($worst | short) end) as $head
    | ([$u.windows[] | select(.p >= 80)]) as $acute
    | (if $limited then $head
       else ([$acute[] | short] | join("  ")) + (if $worst.p >= 90 then "  󰔟 \(countdown($worst.reset))" else "" end) end) as $alert
    | (if $mode == "detail-head" then
         [($u.windows[] | select(. != $worst)), ($u.models[] | select(.p > 0))]
       elif $mode == "detail-alert" then
         [($u.windows[] | select(if $limited then .p < 100 else .p < 80 end))]
       else $u.windows end) as $parts
    | ([$parts[] | shown] | join("  ")) as $detail
    | (if ($mode | startswith("detail")) then ($parts | map(.p) | max // 0) else $worst.p end) as $level
    | ([($u.windows + $u.models)[]
        | "\(.label | esc): \(.p)%" + (if .reset != null then ", resets \(countdown(.reset))" else "" end)]) as $rows
    | ("<b>\($u.name)</b>" + (if $u.plan != "" then " (\($u.plan | esc))" else "" end)
       + (if $u.stale != null then ", \($u.stale) min old" else "" end)
       + "\n" + ($rows | join("\n"))
       + (if $limited then "\nLimit reached: back \(countdown($back))" else "" end)
       + (if $u.problem != "" then "\n\($u.problem | esc)" else "" end)) as $tooltip
    | (if $mode == "head" or $mode == "show" then $head
       elif $mode == "alert" then $alert
       else $detail end) as $text
    | {text: (if $text == "" then "" elif $mode == "detail-head" then $text else $mark + $text end),
       tooltip: $tooltip,
       class: (if $text == "" then ["idle", $u.agent]
               else [class($level; $limited and ($mode | startswith("detail") | not)), $u.agent] + (if $logo then ["logo"] else [] end) end),
       percentage: ($worst.p | if . > 100 then 100 else . end)}
  '
}

# The tooltip lines of the other agents for the head.
others_tooltip() {
  for other in "$@"; do
    [ "$other" = "$agent" ] && continue
    tip=$(normalized "$other" | render "$other" show | jq -r '.tooltip' 2>/dev/null)
    [ -n "$tip" ] && printf '\n%s\n' "$tip"
  done
}

case $mode in
  refresh)
    if logged_in "$agent"; then fetch "$agent" 1; fi
    pkill -RTMIN+$signal -x waybar 2>/dev/null
    exit 0
    ;;
  open)
    case $agent in
      claude) url=https://claude.ai/settings/usage ;;
      codex) url=https://chatgpt.com/codex/settings/usage ;;
      *) exit 0 ;;
    esac
    exec xdg-open "$url"
    ;;
  show)
    logged_in "$agent" || empty "Not logged in to $agent"
    fetch "$agent" 0
    usage=$(normalized "$agent") || empty "No usage of $agent yet"
    printf '%s' "$usage" | render "$agent" show
    ;;
  head)
    order="$agent $*"
    # shellcheck disable=SC2086 # the agents, one word each
    agent=$(head_of $order)
    [ -n "$agent" ] || empty "No agent logged in"
    fetch "$agent" 0
    usage=$(normalized "$agent") || empty "No usage of $agent yet"
    # shellcheck disable=SC2086
    more=$(others_tooltip $order)
    printf '%s' "$usage" | render "$agent" head |
      jq -c --arg more "$more" 'if $more != "" then .tooltip += "\n" + $more else . end'
    ;;
  alert | detail)
    # shellcheck disable=SC2068 # the agents, one word each
    head=$(head_of $@)
    logged_in "$agent" || empty
    [ "$mode" = alert ] && [ "$agent" = "$head" ] && empty
    [ "$mode" = alert ] && fetch "$agent" 0
    usage=$(normalized "$agent") || empty
    if [ "$mode" = detail ]; then
      if [ "$agent" = "$head" ]; then
        mode=detail-head
      elif [ -n "$(printf '%s' "$usage" | render "$agent" alert | jq -r '.text')" ]; then
        mode=detail-alert
      fi
    fi
    printf '%s' "$usage" | render "$agent" "$mode"
    ;;
  *)
    empty "Unknown mode: $mode"
    ;;
esac
