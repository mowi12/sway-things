#!/bin/sh
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
# Sway Things template: a waybar custom module. It is yours once copied; waybar runs it.
#
# Shows nothing; warns with notify-send once when the battery falls to 15 % and again at 5 %
# while it discharges. Run it every minute.
set -u

if [ -z "${XDG_RUNTIME_DIR:-}" ]; then
  printf '{"text": "", "tooltip": "This module needs XDG_RUNTIME_DIR, which the session sets"}\n'
  exit 0
fi
warned=$XDG_RUNTIME_DIR/sway-things-battery-warned
capacity="" status=""
for battery in /sys/class/power_supply/*; do
  [ "$(cat "$battery/type" 2>/dev/null)" = Battery ] || continue
  capacity=$(cat "$battery/capacity" 2>/dev/null)
  status=$(cat "$battery/status" 2>/dev/null)
  break
done
if [ -n "$capacity" ] && [ "$status" = Discharging ]; then
  last=$(cat "$warned" 2>/dev/null || echo 100)
  for level in 15 5; do
    if [ "$capacity" -le "$level" ] && [ "$last" -gt "$level" ]; then
      urgency=normal
      [ "$level" -le 5 ] && urgency=critical
      notify-send -u "$urgency" "Battery low" "$capacity % left" 2>/dev/null
      echo "$level" >"$warned"
    fi
  done
else
  rm -f "$warned"
fi
printf '{"text": ""}\n'
