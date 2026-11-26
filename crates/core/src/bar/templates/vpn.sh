#!/bin/sh
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
# Sway Things template: a waybar custom module. It is yours once copied; waybar runs it.
#
# The VPN and WireGuard connections NetworkManager has up, hidden while none is. Needs nmcli.
# Run it every half minute.
set -u

active=$(nmcli -t -f NAME,TYPE connection show --active 2>/dev/null |
  awk -F: '$2 == "vpn" || $2 == "wireguard" { printf "%s%s", sep, $1; sep = ", " }')
if [ -n "$active" ]; then
  printf '{"text": "🔒", "tooltip": "VPN: %s", "class": "up"}\n' "$(printf '%s' "$active" | sed 's/[\\"]/\\&/g')"
else
  printf '{"text": "", "class": "down"}\n'
fi
