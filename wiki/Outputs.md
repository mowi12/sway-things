# Outputs

> Part of the [architecture](Architecture.md). Displays: reading the outputs, the arrangement, display profiles with
> kanshi, workspace placement and mirroring.

`crates/core/src/outputs/` is the list of outputs for the Displays page, per-output settings and the lockout protection.
The Cursor & Tablet page offers its connected outputs as the screens a drawing tablet or a touchscreen can map to.

## The output list

- **From sway:** each output has its name (the connector, `DP-4`), make, model and serial, whether it is on, the modes
  it offers, and while it is on the mode, the place and size in the layout (logical pixels), the scale and the
  transform. Outputs that aren't desktops (VR headsets) are left out. All of these strings are external.
- **Identifier:** settings are stored for an output by make, model and serial as sway joins them for `output` commands
  (`<make> <model> <serial>`, `Unknown` for what the output doesn't report), which stays the same when a dock or another
  port gives the output another name. An output without a serial (`Unknown`, like a headless output or many laptop
  panels) is stored by its name instead: two monitors of one model would share the identifier, and which one is plugged
  in would decide what it stands for. So is an output whose identifier another connected output reports too (a broken
  serial; once one of them is gone, the other goes back to the identifier) or that is longer than the 127 bytes sway
  keeps; values stored by such an identifier stay listed as an output that isn't connected, where they can be removed.
  Values stored by a name stay with the port: a monitor without a serial moved to another port starts without them. The
  command line takes a display by its name too: in a session, `--device DP-4` stands for that display's identifier; a
  value stored by a name is reachable by it while nothing is connected on that port.
- **Positions are absolute:** a drop stores the position of every output that is on, so an output plugged in later with
  other outputs (another dock) comes back where it stood and may not touch them; the page then offers _Arrange Side by
  Side_. Arrangements per combination of outputs are for display profiles (kanshi).
- **Hotplug without polling:** `OutputWatch` subscribes to sway's `output` events on a connection of its own and reads
  `GET_OUTPUTS` again after each one. sway sends an event without details for every change of the layout, so only real
  changes are reported (added, removed, changed).
- **Stored but not connected:** an output with values in the settings files stays listed as disconnected, by its
  identifier.
- **What sway chose:** `Output::session_value(key)` is what the output runs with for a Displays setting, as the setting
  stores it. A row shows it where nothing sets the value (`Source::Session`), and the safety net restores it.
- **Last output:** `Outputs::lockout(output, enabled)` says whether switching an output (by its identifier or name) off
  would leave none on. Switching every output at once isn't offered: it is one at a time.
- **Names and modes:** `Output::label` is make and model, `Output::brand` the brand people know (_LG_ for "Goldstar"),
  `Output::is_built_in` a laptop's panel by its connector; the canvas, the picker and the workspace rule use them.
  `outputs::modes` groups the modes as the mode row offers them: a mode per size (`sizes`, the mode in use standing for
  its size), then the rates of one size (`rates`); the page only adds the labels.

## Display profiles with kanshi

### When kanshi is in charge

`generate::Displays::detect` makes kanshi the display backend when it is installed (`kanshi` in `$PATH`) and its config
includes ours, or there is none (it is then created with the include line); `Displays::with_profiles` goes back to sway
while `displays.toml` can't be read, for the app, the command line and `doctor` alike. Our `exec kanshi` is dropped once
a kanshi runs as a unit of its own (its control group is named after it). Nothing is run. The app looks at most every
two seconds (`Model::look_for_kanshi`, which reads `/proc` and kanshi's config on a thread of its own and keeps what it
saw for the page) and watches kanshi's config and ours, so an include line added or moved shows at once, and has kanshi
read its config again then. A model made for tests never looks.

### Following the connected displays

With kanshi, per-output settings are stored as always, so a row shows exactly the value set (never a value sway reports
back, like a scale of 1.2999999523, which a control would send again) and the canvas flows the layout along with a new
size. They stand for the combination connected now: `Applier::follow_combination` (the app runs it whenever sway reports
a change of the outputs, the command line before a change of a display) replaces them with the profile of a combination
that comes back, or with what the displays run with for one never set up here, and drops the values of displays that
aren't connected and that a remembered combination holds (those of a display set up before kanshi did it stay). For ten
seconds after the combination came, while kanshi sets it up, and until a setting of the displays changes here, they keep
following; a change from elsewhere after that is left as it is. A change made here and not saved yet when the
combination changes is remembered for the combination it was made for first. Following never runs while a risky change
waits, and isn't saved on its own (a save per dock and undock would push the user's changes out of the history). A value
for every display is stored for each one connected, and one stored for every display before kanshi set them up is
dropped then (a profile takes it for a display without its own meanwhile). Without a session, a display's setting isn't
changed at all (`ApplyError::DisplaysNeedSession`): it would reach neither sway's lines nor a profile. Nothing is sent:
kanshi sets the displays up.

### Remembering the profiles

When the settings are saved after a per-output setting changed (`Applier::touch`) and no risky change waits,
`Applier::remember_displays` makes the profile of the connected outputs from the stored values
(`outputs::profiles::Profile::stored`, what they run with where nothing is stored) and remembers it for their
combination in `displays.toml` next to the settings files (`Profiles`), unless the combination changed since the values
were made for it, writes `~/.config/kanshi/sway-things.conf` from all of them (`kanshi_config`: a profile per
combination named `sway-things-<hash>`, then for each display of them without a remembered combination of its own, one
that switches it on when it is the only one connected (a laptop's panel switched off at the dock comes back when the
dock goes). There is no profile for any displays (`...output *`): kanshi keeps a profile that still matches when
displays are plugged in, so it would never switch to a remembered combination again (the profiles with exactly one
display more, which ask about it, are described in [A new display asks once](#a-new-display-asks-once)); a combination
with every display off is never remembered nor written) and has kanshi read it again (`kanshictl reload`, five seconds)
if the file changed. If our include starts kanshi and none runs yet (the first time), sway starts it now (`exec kanshi`,
once per run of the app), not only at the next login. sway reports a scale as a 32-bit float; `Output` rounds it to six
places, which is the scale that was set.

### The files

`outputs::kanshi::KanshiFiles` writes only a file that starts with our header or is empty (people make an empty one so
kanshi starts), and never the user's config except to create it. kanshi rejects its whole config over an include it
can't find, so the file is kept there and current at every save (`Applier::keep_kanshi_file`), also when no display was
ever set up, and `hook displays` writes it at every start and reload of sway and starts kanshi if it gave up over it.
Each profile says in comment lines which workspaces go on which display by the rule (`Profile::placement_lines`, from
`workspaces::place_on`, the same placement the hook makes), above the command that places them, so the file shows what
is set. `Applier::forget_displays` (the page's reset, while kanshi sets up the displays) drops the profile of the
displays connected with their stored values, pictures and placement and has kanshi read the file again, so what set them
up before applies again: a profile of the user's own, or what they run with.

### sway's lines and the hook

`20-output.conf` then has no per-output lines, a comment, the startup guard (always with kanshi), and `exec kanshi` when
the user's sway config doesn't start kanshi (`kanshi::started_by`, not counting our files) and no kanshi started
elsewhere (a systemd unit) runs; a kanshi our own line started keeps being started. `displays.toml` starts with our
header and a file without it is never written over; one that can't be read isn't either: the displays are set up from
our sway lines meanwhile, and the app says why. `hook displays` has kanshi apply its profile again and then makes sure a
display is on. The app looks again before each change and when displays come and go (at most every two seconds) and
writes the files when the answer changes; `doctor` compares against the same and checks no conflicts of per-output
settings meanwhile.

### The user's own kanshi profiles

A combination whose displays the user's kanshi profiles set up can still have its workspaces placed (or a picture
mirrored) without Sway Things taking over the displays. While a rule places or a combination mirrors,
`KanshiFiles::config_for` copies every profile of the user's config after our include into our file
(`kanshi::after_ours`, read by `outputs::kanshi_config`, which reads words as kanshi's scfg does: `"…"` with `\`
escapes, `'…'`, `\` outside quotes), after our arranged profiles and before the profiles of a display alone
(`profiles::kanshi_config_with`). kanshi tries our file first, so the copy applies in place of the original: the user's
`output` lines and every other line as written, without a hook of ours, with `exec sway-things hook workspaces` (and
`hook mirror`) added, under a comment that names the original. While a rule places, the lines that do nothing but place
workspaces are left out too (`Kind::PlacesWorkspaces`: a plain `swaymsg` whose every command is `workspace N …`,
`move workspace to …` or `focus output …`); a line that does more, such as setting up a display in the same `swaymsg` or
running through the shell, is kept. A copy is named `sway-things-of-<hash>` of the profile's name and displays, so it
stays the same while the profile does. Aliases and defaults the user's config defines after the include work in our file
too (kanshi reads the whole config first).

The copies stay current: the app watches the user's config, and `hook displays` (at every start and reload of sway) and
`hook workspaces` (after placing) write our file again, so an edit takes effect without the app at the latest with the
next reload. A placement made on the map of a combination that a profile of the user's sets up (`KanshiFiles::sets_up`)
is remembered as `placement_only` in `displays.toml`: its workspaces, but no display setup, so it never makes our
profile win over the user's (`Profiles::arranged` are the others; `forget` keeps a profile with workspaces as placement
only). Elsewhere it is remembered with the displays as they are, so a profile of ours brings it back. The Displays page
then says the user's profile sets up the displays and Sway Things places the workspaces. `doctor` warns of an `exec`
line kept in a copy that may move workspaces (one with `swaymsg` and `workspace` that does more, or a program named
after workspaces), and of an include after ours whose file may hold profiles, which aren't copied
(`kanshi_profile_copy`). Without a rule or a mirror, nothing is copied and our file is as before.

### What doctor and the page report

`doctor` also checks that `sway-things` is in `$PATH` and is this build (`hook_program`), since sway and kanshi run the
hooks by that name. `doctor` reports the file missing, out of date or someone else's; the Displays page and
`sway-things set` say when it is someone else's, since nothing set reaches kanshi then. With kanshi installed, `doctor`
reports a kanshi config without our include or with something before it (with `KanshiFiles::how_to_include`: in words,
to put the include line at the top, above the user's profiles, and take a line further down that includes ours away;
Sway Things gives no command to paste, since the user changes their own file), a file of someone else's in the place of
ours, and a profile of the user's before the include that covers a remembered combination (`kanshi::user_profiles`, with
`output *` as one more display and `...output *` as any number), since kanshi applies the first that matches. A file
included before ours isn't read: it counts as standing above the line. The Displays page and `sway-things set` warn of a
profile that wins for the displays connected now (`kanshi::overriding`), and the page of anything above the line. The
rows of the displays have no reset button and no machine toggle meanwhile: the values stand for the combination, not for
every machine.

## Workspace placement

`outputs::workspaces` puts workspaces `1` to `workspaces.count` on the displays by the rule `workspaces.placement`
(`Rule`: sway's own, round robin, blocks, the built-in display, or fixed). Both settings are on the Workspaces page and
have no sway directive, since the placement depends on the displays connected. `place` gives each workspace its display
(by its name in the settings), leaving out displays that show another one's picture; a fixed placement is remembered per
combination in `displays.toml` (`[[profile]] workspaces`), and what it misses is placed round robin. `with_fallbacks`
adds the other displays from left to right, and `placing` declares `workspace N output A B C` with the connected ones,
so a workspace whose display goes away moves to the next one and back when it returns. It also moves an open workspace
that stands elsewhere (`workspace --no-auto-back-and-forth N`, `move workspace to output A`), since sway's declaration
only appends to a list that is never cleared, then gives the focus back. `apply` does all that for the session; it runs
from `sway-things hook workspaces`, which every kanshi profile runs while a rule places (kanshi's file is written again
when the rule changes) and `80-hooks.conf` runs at every start and reload of sway while a rule places (sway forgets the
declarations then), and from the applier after a change of either setting. The hook first waits for the displays to
stand (`workspaces::settled`: where our profile of the combination puts them, or still for 0.7 seconds, at most four
seconds), since after a start or reload kanshi puts them in place a moment later and "left to right" read before that
would be another placement, which sway then keeps. After placing, `showing_own` has each display show a workspace of its
own: sway gives a display whose workspace moved away the next free number, which may be another display's, and that
number would then lead there; a display with none of its own (all on one display) shows the first number after the
placed ones. A click or drag on the map sends one move (`Applier::fix_placement`): it is made on the placement as it is
then, so fast moves all count, remembered for the combination, and makes the rule fixed, so the workspaces are placed
once. Both settings apply after a reload (`Apply::Reload`, which a setting without a sway directive honors with
`Effect::AfterReload`, and so does a move on the map): sway keeps a session's declarations in order, so open workspaces
move at once, but one closed and opened again goes to its old display until sway reloads.

## Mirroring

`outputs::mirror` keeps, per display of a combination, which display it shows instead of its own content and how it fits
(`mirror`, `mirror_scaling` in `displays.toml`); `Profile::set_mirror` refuses chains and cycles (a source that shows
another display, a target shown elsewhere), and so does the page. `apply` is idempotent: it reads the `wl-mirror`
processes Sway Things started in this session (found in `/proc` by the window title _Sway Things mirror_ and the
session's `XDG_RUNTIME_DIR`/`WAYLAND_DISPLAY`), ends those the combination connected now doesn't ask for (`kill` with
their numbers) and starts only the missing ones
(`setsid -f wl-mirror --fullscreen-output <display> --scaling <fit|cover|exact> --title … <source>`), with argument
lists and no shell. Once any combination mirrors, every kanshi profile, those of a display alone too, runs
`exec sway-things hook mirror`, and `hook displays` runs it after a start or reload of sway. Whatever a mirror takes
from its display is taken only for a mirror that can start (`mirror::startable`, the one rule for `apply`, the
workspaces and the place apart: `wl-mirror` installed, both displays on, no chain or cycle), so a picture that can't
start leaves its display as it is; `kanshi_config` leaves the place apart out without `wl-mirror`, and the hook writes
kanshi's file again when that changed. A display that mirrors stands apart (`Profile::apart`: right of the others' right
edge, worked out from mode, turn and scale, plus `APART` = 2000 logical pixels, which the pointer doesn't cross), in the
kanshi profile and live, with a workspace of its own for the picture (`mirror::workspace_name`). It takes no workspaces
(also not from the placement, which sends them to the display shown): `mirror::gather_workspaces` moves the workspaces
with windows on it to the display shown before its mirror starts, and again each time the hook runs (a workspace the
user's config or another tool put there since would hide the picture, out of the pointer's reach), puts the display back
apart if something set it among the others, and notes the workspaces in the profile (`mirror_workspaces`, with the sway
session as `mirror_session`, so a later login gives nothing back), which `give_back_workspaces` moves back, with the
display back at its place, when it shows its own content again; `mirror::leave_ended` has a display whose picture ended
show a workspace like any other again, so none named after a picture stays behind; the mirror's window doesn't take the
focus (`no_focus` by its title, in `20-output.conf` and sent before a mirror starts; a fullscreen window that takes it
anyway gives it back, `mirror::apply_keeping_focus`). The app reads `displays.toml` again before it changes it, since
the hook may have noted something meanwhile.

### A picture that went comes back

Without a daemon: `mirror::missing` tells a picture the combination connected now asks for whose `wl-mirror` isn't
running (crashed, its window closed) from the `/proc` entries, and `mirror::heal` starts it again as `mirror::start`
does (gather the workspaces, `no_focus`, `apply`, `leave_ended`), sending nothing while every picture runs and ending no
picture of another combination. `start` runs one at a time in a session (the lock `sway-things-mirror-start.lock`,
waited for at most fifteen seconds) and reads `displays.toml` again once it has it, so a hook's save never drops what
another one noted. It runs where Sway Things runs anyway: `hook workspaces` (kanshi's profiles and every start and
reload of sway while a rule places), `hook displays` and `hook mirror` (which start them as before), and the app
(`Applier::heal_mirrors`, queued as `Op::HealMirrors` when it opens and three seconds after the last change of the
outputs, `HEAL_AFTER`, so kanshi's own hooks went first). `mirror::apply` takes a lock in `$XDG_RUNTIME_DIR`
(`outputs::SessionLock`), so two of them never start a picture twice. A `SessionLock` is a file made with `create_new`
holding a token (process number, time, counter); `Drop` removes it only while it still holds that token. One whose
process is gone (or older than ten minutes) is taken over by moving it aside first, which only one process can do, and
put back with a hard link if what was moved was a newer lock. The focus goes back only if it is on a picture's workspace
and the workspace that had it is still there; looking stops as soon as the pictures' windows are shown (and 0.3 seconds
later), or the user went to another workspace, at most after three seconds.

## A new display asks once

For each arranged combination, `profiles::kanshi_config_with` (with `ask`) adds a profile `sway-things-new-<hash>` with
the combination's displays without directives (nothing of their setup changes) and `output *`, which kanshi matches with
exactly one display more, running `exec sway-things hook new-display`. It comes after our arranged profiles and the
copies of the user's (which win for their displays) and before the profiles of a display alone; kanshi tries the exact
profile of a remembered combination first. `KanshiFiles::config_for` asks only where `wl-mirror` is installed and no
include after ours may hold profiles that can't be read; a combination for which a profile of the user's after our
include may set up one display more has none (`may_take_one_more`: the same number of displays, or `...output *`, and at
most one criterion that surely names another display: one with make, model and serial whose pattern matches none of
them; a connector, an alias or a pattern with `[` may be one of them).

`hook new-display` (`outputs::hooks::new_display`) waits until the outputs stand (unchanged for 1.5 seconds, at most
ten: a dock's displays come one after the other), leaves a combination a profile of the user's sets up
(`KanshiFiles::sets_up`) alone, and asks `outputs::new_display::decide`: a question only for exactly one display that
isn't in any remembered combination (`Profiles::seen`), is on, and was added to an arranged combination with a display
that can be shown (on, showing its own content: the built-in panel, else the focused one, else the leftmost). A display
seen before that showed a picture in a remembered combination shows it again if that display is there
(`Decision::Repeat`); else nothing. The question is noted in `$XDG_RUNTIME_DIR/sway-things-new-display` with the sway
session and the output's `id` (new at each plug-in), so kanshi applying the profile again (a reload) asks no second
time, and while it waits the hook holds a lock per session and display (`new_display::asking`), so the display plugged
in again meanwhile isn't asked about twice (the first answer counts, for the same combination). kanshi doesn't wait for
`exec`, so the waiting hook holds up nothing. Without a notification server, or one that doesn't list `actions`
(`Asked::NoActions`), the hook writes one line for kanshi's log and ends with success; the whole question gives up ten
seconds after `WAIT` if the bus doesn't answer.

`new_display::ask` sends a notification over `org.freedesktop.Notifications` (`GetCapabilities` must list `actions`)
with the actions `default` and `mirror` (_Mirror_) and `extend` (_Extend_), the hint `desktop-entry` and a timeout of
two minutes (`WAIT`), and waits for `ActionInvoked` or `NotificationClosed` of its id, subscribed before it is sent;
then it closes the notification. The answer becomes the profile of the new combination (`Profile::with_one_more`: the
known combination as remembered, the new display as it runs, with a `fit` picture of the source for _Mirror_), saved to
`displays.toml` unless the displays changed or the combination was remembered meanwhile; kanshi's file is written again
and kanshi reloads it, which applies the new profile (`kanshictl reload` applies the first matching profile anew), and
the pictures start (`mirror::start`). No answer changes nothing.

## Never without a display

An output stored as off stays off when sway starts, also when it is the only one connected then (a laptop's panel,
switched off at the dock, after starting without the dock); sway has no protection of its own. So while Sway Things
stores anything for an output (a line of the user's may still switch it off), `20-output.conf` ends with
`exec_always sway-things hook displays`, and at every start and reload `outputs::ensure_one_on` switches the first
connected output on if none is on (for the session only). While none is on, it waits until sway has reported no change
of its outputs for three seconds (at a login at the dock, the panel comes before the dock's monitors), but no longer
than a minute: while sway reports no output at all, it then gives up, and while sway keeps reporting changes, it
switches one on anyway. The row of a display stored as off that the hook switched on says _On for now: no other display
was on_. Without a session the outputs aren't known, so switching one off is allowed there; the hook catches what that
leaves. Undocking in a running session isn't covered: sway has no config hook for an output that goes; display profiles
(kanshi) are the place for that.

## Arranging

`outputs::layout` is the geometry of the Displays canvas, without GTK. `logical_size` is an output's size in the layout
(the mode turned by the transform, divided by the scale and rounded down, as wlroots does). While an output is dragged,
`snap` pulls its edges and centers to those of the others within a threshold and lets go beyond it. When it is dropped,
`settle` moves it to the nearest place where it overlaps no other output and shares at least a quarter of an edge with
one, since sway only moves the pointer between outputs that share an edge, preferring places where the whole layout
hangs together. `reflow` keeps a layout together when outputs change their size. `slot_at` finds the edge two touching
outputs share that a dragged one points at (with some hysteresis, so the marker doesn't flicker), and `insert` puts it
there, moving what stands behind in that row (with what is stacked on it) by its size. `place` is a drop: into a slot,
else `settle`; moved away from where it stood, `close_gap` first closes the gap it left in its row or column, and no
other. `normalize` moves the layout to start at 0,0.
