#!/bin/sh
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
# Sway Things template: a waybar custom module. It is yours once copied; waybar runs it.
#
# Shows a camera icon while any program has a camera open (/dev/video*), with the programs in the
# tooltip, and nothing otherwise. Unlike waybar's privacy module it also sees programs that open
# the camera directly instead of through PipeWire. Needs fuser (psmisc). Run it every few seconds.
set -u

programs=""
for device in /dev/video*; do
  [ -e "$device" ] || continue
  for pid in $(fuser "$device" 2>/dev/null); do
    name=$(cat "/proc/$pid/comm" 2>/dev/null) || continue
    case " $programs " in
      *" $name "*) ;;
      *) programs="$programs $name" ;;
    esac
  done
done
programs=${programs# }
if [ -n "$programs" ]; then
  # Program names are anyone's: no control characters, and quotes and backslashes escaped for
  # JSON.
  programs=$(printf '%s' "$programs" | tr -d '[:cntrl:]' | sed 's/[\\"]/\\&/g')
  printf '{"text": "📷", "tooltip": "Camera in use: %s", "class": "on"}\n' "$programs"
else
  printf '{"text": "", "class": "off"}\n'
fi
