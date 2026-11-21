#!/bin/sh
# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later
# Sway Things template: a waybar custom module. It is yours once copied; waybar runs it.
#
# A pomodoro timer: work, then a short break, and a long break after every fourth work phase.
#
#   pomodoro.sh          JSON for waybar (run it every second)
#   pomodoro.sh toggle   start, pause or go on       (click)
#   pomodoro.sh reset    stop, back to the first round (right click)
#   pomodoro.sh skip     on to the next phase          (middle click)
#   pomodoro.sh up|down  five minutes more or less work while stopped (scroll)
#
# At the end of a phase it says so with notify-send, if installed, and waits for a click. The
# state lives in $XDG_STATE_HOME/sway-things-pomodoro.
set -u

state=${XDG_STATE_HOME:-$HOME/.local/state}/sway-things-pomodoro
phase=work status=idle end=0 left=1500 work=25 short=5 long=15 rounds=0
# The state is read as key=value words, never run: only known keys with fitting values count.
if [ -r "$state" ]; then
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
    esac
  done
fi
now=$(date +%s)

duration() {
  case $phase in
    work) echo $((work * 60)) ;;
    short) echo $((short * 60)) ;;
    *) echo $((long * 60)) ;;
  esac
}

save() {
  mkdir -p "${state%/*}"
  umask 077
  printf 'phase=%s status=%s end=%s left=%s work=%s short=%s long=%s rounds=%s\n' \
    "$phase" "$status" "$end" "$left" "$work" "$short" "$long" "$rounds" >"$state.tmp" &&
    mv "$state.tmp" "$state"
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

case ${1:-} in
  toggle)
    case $status in
      run) status=pause left=$((end - now)) ;;
      *) status=run end=$((now + left)) ;;
    esac
    save
    exit 0
    ;;
  reset)
    phase=work status=idle rounds=0
    left=$(duration)
    save
    exit 0
    ;;
  skip)
    next_phase
    save
    exit 0
    ;;
  up | down)
    if [ "$status" = idle ] && [ "$phase" = work ]; then
      if [ "$1" = up ] && [ "$work" -lt 90 ]; then work=$((work + 5)); fi
      if [ "$1" = down ] && [ "$work" -gt 5 ]; then work=$((work - 5)); fi
      left=$(duration)
      save
    fi
    exit 0
    ;;
esac

if [ "$status" = run ] && [ "$now" -ge "$end" ]; then
  ended=$phase
  next_phase
  save
  if command -v notify-send >/dev/null; then
    notify-send "Pomodoro" "The $ended phase is over. Click the timer to go on."
  fi
fi
[ "$status" = run ] && left=$((end - now))
[ "$left" -lt 0 ] && left=0
time=$(printf '%02d:%02d' $((left / 60)) $((left % 60)))
case $status in
  idle) text="⏱ $time" ;;
  pause) text="⏸ $time" ;;
  *) text="▶ $time" ;;
esac
printf '{"text": "%s", "tooltip": "%s, round %s", "class": ["%s", "%s"]}\n' \
  "$text" "$phase" "$((rounds + 1))" "$phase" "$status"
