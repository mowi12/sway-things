#!/bin/sh
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-FileCopyrightText: 2026 nurkert
# SPDX-License-Identifier: GPL-3.0-or-later
# Sway Things template: a waybar custom module. It is yours once copied; waybar runs it.
#
# A pomodoro timer: work, then a short break, and a long break after every fourth work phase.
# When a phase ends it says so (notify-send and a sound, where installed) and waits: the next
# phase starts with a click.
#
#   pomodoro.sh          JSON for waybar (run it every second)
#   pomodoro.sh toggle   start, pause or go on                        (click)
#   pomodoro.sh reset    stop, back to the first round                (right click)
#   pomodoro.sh skip     on to the next phase, stopped                (middle click)
#   pomodoro.sh up|down  the length of the phase shown, while stopped (scroll): work in steps
#                        of 5 minutes (5 to 90), breaks of 1 minute (short, 1 to 30) or 5 (long)
#
# Stopped, it shows only its symbol; running or paused, the time left. Its classes say the
# phase while it runs or is paused (work, short, long) and paused, ending (the last minute);
# stopped, idle or ready (the next phase waits for a click) and next-work, next-short or
# next-long, for the stylesheet. A change redraws the bar at once (signal 10).
# The state lives in $XDG_STATE_HOME/sway-things-pomodoro.
set -u

signal=10
state=${XDG_STATE_HOME:-$HOME/.local/state}/sway-things-pomodoro
phase=work status=idle end=0 left=1500 work=25 short=5 long=15 rounds=0 shown=0

# The state is read as key=value words, never run: only known keys with fitting values count.
load() {
  [ -r "$state" ] || return 0
  read -r saved <"$state" || true
  for pair in ${saved:-}; do
    value=${pair#*=}
    case $pair in
      phase=work | phase=short | phase=long) phase=$value ;;
      status=idle | status=run | status=pause) status=$value ;;
      *=*[!0-9]* | *=) ;;
      end=*) end=$value ;;
      left=*) left=$value ;;
      work=*) work=$value ;;
      short=*) short=$value ;;
      long=*) long=$value ;;
      rounds=*) rounds=$value ;;
      shown=*) shown=$value ;;
    esac
  done
}

save() {
  mkdir -p "${state%/*}"
  printf 'phase=%s status=%s end=%s left=%s work=%s short=%s long=%s rounds=%s shown=%s\n' \
    "$phase" "$status" "$end" "$left" "$work" "$short" "$long" "$rounds" "$shown" \
    >"$state.tmp" && mv "$state.tmp" "$state"
}

redraw() {
  pkill -RTMIN+$signal -x waybar 2>/dev/null
}

duration() {
  case $phase in
    work) echo $((work * 60)) ;;
    short) echo $((short * 60)) ;;
    *) echo $((long * 60)) ;;
  esac
}

next_phase() {
  if [ "$phase" = work ]; then
    rounds=$((rounds + 1))
    if [ $((rounds % 4)) -eq 0 ]; then phase=long; else phase=short; fi
  else
    phase=work
  fi
  status=idle
  left=$(duration)
}

# The number $1 changed by $2, kept within $3 … $4.
clamp() {
  value=$(($1 + $2))
  [ "$value" -lt "$3" ] && value=$3
  [ "$value" -gt "$4" ] && value=$4
  echo "$value"
}

say() {
  if command -v notify-send >/dev/null; then
    notify-send -a Pomodoro -i appointment-soon "$1" "$2" 2>/dev/null
  fi
  sound=/usr/share/sounds/freedesktop/stereo/alarm-clock-elapsed.oga
  if [ -r "$sound" ] && command -v paplay >/dev/null; then
    paplay "$sound" >/dev/null 2>&1 &
  fi
}

umask 077
mkdir -p "${state%/*}"
# waybar runs the module once per display; the end of a phase is told once.
if command -v flock >/dev/null; then
  exec 9>"$state.lock"
  flock -w 5 9 || true
fi
load
now=$(date +%s)

case ${1:-} in
  toggle)
    case $status in
      run) status=pause left=$((end - now)) ;;
      *) status=run end=$((now + left)) ;;
    esac
    save
    redraw
    exit 0
    ;;
  reset)
    phase=work status=idle rounds=0
    left=$(duration)
    save
    redraw
    exit 0
    ;;
  skip)
    next_phase
    save
    redraw
    exit 0
    ;;
  up | down)
    [ "$status" = idle ] || exit 0
    sign=1
    [ "$1" = down ] && sign=-1
    case $phase in
      work) work=$(clamp "$work" $((5 * sign)) 5 90) ;;
      short) short=$(clamp "$short" "$sign" 1 30) ;;
      long) long=$(clamp "$long" $((5 * sign)) 5 60) ;;
    esac
    left=$(duration) shown=$now
    save
    redraw
    exit 0
    ;;
esac

if [ "$status" = run ] && [ "$now" -ge "$end" ]; then
  ended=$phase
  next_phase
  save
  case $ended in
    work) say "Work phase done" "Time for a break of $(($(duration) / 60)) minutes. Click the timer to start it." ;;
    *) say "Break over" "Next: $work minutes of work. Click the timer to start." ;;
  esac
fi
[ "$status" = run ] && left=$((end - now))
[ "$left" -lt 0 ] && left=0
time=$(printf '%02d:%02d' $((left / 60)) $((left % 60)))

# Symbols of the Nerd Fonts: a timer while working, a cup in a break, a pause sign.
case $phase in
  work) symbol='󰔛' name="Work" ;;
  short) symbol='󰅶' name="Short break" ;;
  *) symbol='󰅶' name="Long break" ;;
esac
classes="\"$phase\""
case $status in
  idle)
    waiting=ready
    [ "$phase" = work ] && [ "$rounds" -eq 0 ] && waiting=idle
    classes="\"$waiting\", \"next-$phase\""
    text=$symbol
    [ $((now - shown)) -lt 3 ] && text="$symbol $time"
    doing="stopped"
    [ "$waiting" = ready ] && doing="waiting for a click"
    ;;
  pause)
    symbol='󰏤'
    classes="$classes, \"paused\""
    text="$symbol $time"
    doing="paused"
    ;;
  *)
    text="$symbol $time"
    [ "$left" -le 60 ] && classes="$classes, \"ending\""
    doing="running"
    ;;
esac
printf '{"text": "%s", "class": [%s], "tooltip": "Pomodoro: %s %s, %s\\nRound %s of 4. Work %s min, short break %s min, long break %s min\\n\\nClick: start or pause. Right click: reset\\nMiddle click: next phase. Scroll: length"}\n' \
  "$text" "$classes" "$name" "$time" "$doing" "$((rounds % 4 + 1))" "$work" "$short" "$long"
