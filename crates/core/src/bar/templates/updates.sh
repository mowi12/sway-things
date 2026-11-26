#!/bin/sh
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
# Sway Things template: a waybar custom module. It is yours once copied; waybar runs it.
#
# The number of package updates, hidden while there are none, from the package manager it
# finds. Needs no root. checkupdates (pacman-contrib, Arch) syncs a temporary copy of the package
# databases, which uses the network; apt, dnf (--cacheonly) and zypper (--no-refresh) read the
# package lists as they were last refreshed and use no network. Run it every half hour.
set -u

if command -v checkupdates >/dev/null; then
  count=$(checkupdates --nocolor 2>/dev/null | wc -l)
elif command -v apt >/dev/null; then
  count=$(apt list --upgradable 2>/dev/null | grep -c upgradable)
elif command -v dnf >/dev/null; then
  count=$(dnf -q --cacheonly check-update 2>/dev/null | grep -c '^[[:alnum:]]')
elif command -v zypper >/dev/null; then
  count=$(zypper -q --no-refresh list-updates 2>/dev/null | grep -c '^v ')
else
  count=0
fi
if [ "${count:-0}" -gt 0 ]; then
  printf '{"text": "⬆ %s", "tooltip": "%s updates", "class": "pending"}\n' "$count" "$count"
else
  printf '{"text": "", "class": "none"}\n'
fi
