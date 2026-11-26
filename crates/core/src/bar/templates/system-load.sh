#!/bin/sh
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
# Sway Things template: a waybar custom module. It is yours once copied; waybar runs it.
#
# The highest of processor, memory, temperature and disk use, with the class warning from 70 %
# (70 °C) and critical from 90 %, and all four in the tooltip. Run it every five seconds.
set -u

if [ -z "${XDG_RUNTIME_DIR:-}" ]; then
  printf '{"text": "", "tooltip": "This module needs XDG_RUNTIME_DIR, which the session sets"}\n'
  exit 0
fi
read -r _ user nice system idle iowait irq softirq _ </proc/stat
busy=$((user + nice + system + irq + softirq))
total=$((busy + idle + iowait))
last=$XDG_RUNTIME_DIR/sway-things-cpu
read -r last_busy last_total 2>/dev/null <"$last" || { last_busy=0 last_total=0; }
echo "$busy $total" >"$last"
cpu=0
[ $((total - last_total)) -gt 0 ] && cpu=$(((busy - last_busy) * 100 / (total - last_total)))
memory=$(awk '/^MemTotal/ { t = $2 } /^MemAvailable/ { a = $2 } END { if (t) print int((t - a) * 100 / t); else print 0 }' /proc/meminfo)
temperature=0
for zone in /sys/class/thermal/thermal_zone*/temp; do
  # One zone that can't be read (a sensor that is off) leaves the others.
  raw=$(cat "$zone" 2>/dev/null) || continue
  case $raw in '' | *[!0-9-]*) continue ;; esac
  value=$((raw / 1000))
  [ "$value" -gt "$temperature" ] && temperature=$value
done
disk=$(df -P / | awk 'NR == 2 { sub("%", "", $5); print $5 }')
highest=cpu value=$cpu
[ "$memory" -gt "$value" ] && highest=memory value=$memory
[ "$temperature" -gt "$value" ] && highest=temperature value=$temperature
[ "$disk" -gt "$value" ] && highest=disk value=$disk
class=normal
[ "$value" -ge 70 ] && class=warning
[ "$value" -ge 90 ] && class=critical
unit=%
[ "$highest" = temperature ] && unit=°C
printf '{"text": "%s %s%s", "tooltip": "CPU %s %%\\nMemory %s %%\\nTemperature %s °C\\nDisk %s %%", "class": "%s"}\n' \
  "$highest" "$value" "$unit" "$cpu" "$memory" "$temperature" "$disk" "$class"
