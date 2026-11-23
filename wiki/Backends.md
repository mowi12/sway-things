# Backends

> Part of the [architecture](Architecture.md). The services and tools Sway Things drives besides sway: BlueZ,
> NetworkManager, the sound server, CUPS and GSettings, the tools of Screenshots & Clipboard, the desktop entries,
> default apps and autostart runners of Apps & Autostart, and the planned backends for the companion tools.

The plan (not built yet) is a backend trait for each area that has competing tools. Today the only companion tool Sway
Things drives is kanshi, in `crates/core/src/outputs/kanshi.rs` ([Outputs](Outputs.md)); Bluetooth, the network, sound
and printers are services with no competing stack and use no trait (below).

```rust
// planned
trait Backend {
    fn detect(&self) -> Availability;   // installed? running? version? managed externally by file:line?
    fn read(&self) -> Result<Values>;
    fn apply(&self, change: &Change) -> Result<()>;
    fn generate(&self, state: &State) -> Vec<GeneratedFile>;
}
```

| Area | Backends (reference first); only display profiles, audio, Bluetooth, the network and printers are built |
| --- | --- |
| Display profiles | kanshi, per combination of displays; without kanshi: static `output` lines in `20-output.conf` (no profiles); shikane later |
| Bar | waybar, swaybar (`bar {}` block) |
| Idle / lock | swayidle + swaylock |
| Notifications | swaync, mako, dunst (Do Not Disturb first) |
| Night light | gammastep, wlsunset |
| Wallpaper | swaybg via `output * bg`; swww later |
| Launcher | fuzzel, wofi, rofi(-wayland), tofi |
| Screenshots | grim + slurp, swappy / satty (the command line, see [below](#screenshots-clipboard-and-recording)) |
| Clipboard history | cliphist with `wl-paste --watch`; a launcher (fuzzel, wofi, rofi, tofi, bemenu) to pick |
| Screen recording | wf-recorder, wl-screenrec |
| Auto dark mode | darkman |
| Audio | PulseAudio API via pipewire-pulse or PulseAudio (`libpulse-binding`, on libpulse's threaded main loop) |
| Autostart | our `exec` lines + hand-written `exec` (read-only) + XDG autostart with runner detection (systemd's `xdg-desktop-autostart.target`, dex), see [Apps and autostart](#apps-and-autostart) |
| Bluetooth | BlueZ (D-Bus) |
| Network | NetworkManager (D-Bus) |
| Printers | CUPS (IPP over its domain socket); cups-pk-helper (D-Bus, polkit); Avahi (D-Bus) |
| Power | power-profiles-daemon / tuned-ppd, UPower (D-Bus) |
| Brightness | logind `Session.SetBrightness` (no root needed) / brightnessctl; ddcutil for external monitors |
| Time / locale | systemd-timedated, systemd-localed (D-Bus, polkit) |

Bluetooth needs no `Backend`: it is runtime state that BlueZ owns and keeps itself, with nothing to store or generate,
and no competing stack. `crates/core/src/bluetooth/` has a `Bluez` handle on the system bus (zbus, async-io): one
`GetManagedObjects` call gives a snapshot of the adapters and devices, the actions are D-Bus calls, and a stream of
`PropertiesChanged`, `InterfacesAdded`/`Removed` and `NameOwnerChanged` signals (without the noise of signal strength)
tells the page to read a new snapshot. Pairing registers our agent (`org.bluez.Agent1`, capability `KeyboardDisplay`)
before each `Pair`, without `RequestDefaultAgent`, so BlueZ asks it only about our pairings; it answers only calls from
the owner of `org.bluez` and hands each question to the page as a prompt. The page runs `StartDiscovery` while it is on
screen and the radio is on, or with `app.bluetooth_search = "manual"` only for 30 s after _Search_. `bluetooth::rfkill`
reads `/dev/rfkill` for the soft and hard block and lifts a soft block by writing one `RFKILL_OP_CHANGE_ALL` event. A
page's "Advanced…" button starts a specialist tool from `delegate` (blueman-manager), found in `PATH`.

The network needs none either: NetworkManager owns the connections and their passwords. `crates/core/src/network/` has a
`NetworkManager` handle on the system bus: `GetManagedObjects` on `/org/freedesktop` and `GetSettings` of each saved
connection give a snapshot of the devices (with their IP configuration), access points, saved and active connections;
Wi-Fi networks are the access points of one SSID merged, sorted by signal level (the four bars) so small changes don't
reorder them. The actions are D-Bus calls: `WirelessEnabled` and `Enable` (polkit), `RequestScan`, `ActivateConnection`
(a saved connection, the best one for a device with "/", a VPN), `AddAndActivateConnection` for a new or hidden network,
`Disconnect`, `DeactivateConnection` and `Delete`. The page asks for a scan every 15 s while it is on screen and Wi-Fi
is on, or with `app.wifi_search = "manual"` for one after _Search_, which waits until the device's `LastScan` moves on
(at most 15 s). A new connection is built as `nmcli device wifi connect` builds it: no `connection.permissions` (every
user), the password with `psk-flags` 0 (kept by NetworkManager), `key-mgmt` from the access point's flags (`wpa-psk`,
`sae`, `owe`). `wait_connected` follows an active connection until it is up, or reports why it ended: the device's state
reason, or for a VPN the reason of its `StateChanged` signal. The change stream takes every signal of NetworkManager
under `/org/freedesktop` and `NameOwnerChanged`, without traffic counters, `LastSeen`/`LastScan` and NetworkManager's
old duplicate `PropertiesChanged` signals. Our secret agent (`org.freedesktop.NetworkManager.SecretAgent` at its fixed
path, registered as `io.github.mowi12.SwayThings` with the capability `VPN_HINTS`) is registered while the page is on
screen and until a connection it started has finished, and again when the owner of `org.freedesktop.NetworkManager`
changes (NetworkManager forgets agents when it restarts). It answers only NetworkManager: for WPA and WPA3 Personal
passwords when it may ask the user, and for the `vpn` secrets of a VPN connection through its plugin's auth dialog
(`network::vpn`). The plugin's `.name` file is looked up as libnm does (`NM_VPN_PLUGIN_DIR`,
`/usr/lib/NetworkManager/VPN`, `/etc/NetworkManager/VPN`; only files of root or the user that nobody else may write),
matched by `service` or `aliases`; its `[GNOME]` group names the `auth-dialog` (a bare name is in `/usr/libexec`) and
whether it supports `--external-ui-mode` and hints. The dialog is started with an argument array as GNOME Shell starts
it (`-u` UUID, `-n` name, `-s` service, `-i` when it may ask, `-r` when the old secrets failed, `-t` per hint), reads
the VPN's data and secrets from stdin and answers on stdout: key and value lines up to an empty line, then it gets
`QUIT` and ends by itself; in external UI mode a key file whose fields with `ShouldAsk` the page asks for in a dialog of
its own. `CancelGetSecrets` ends a running dialog. OpenConnect VPNs don't use the plugin's dialog, which fails for some
servers: the agent signs in itself (`network::openconnect`), as nmcli does. It runs `openconnect --authenticate` (found
in `/usr/sbin`, `/sbin`, `/usr/local/sbin`, then the `bin` directories) in the C.UTF-8 locale with the profile's
settings as `--option=value` (`protocol`, `useragent`, `reported_os`, certificates, proxy, token mode) and the server
after `--`, plus `--external-browser=xdg-open` for web sign-ins. openconnect asks one field at a time on stderr, a
prompt without a line break (ending in a colon, or followed by half a second of silence) and a line read from stdin; its
progress lines are recognized by their beginnings, and the other lines are the server's message for the next prompt. A
field is secret unless its label names a user, account or group; `LABEL [a|b]:` is a choice. Each prompt is a step of a
`SignIn`, which the page shows in one dialog; an answer has its line breaks taken out. The `COOKIE`, `CONNECT_URL` (else
`HOST`), `FINGERPRINT` and `RESOLVE` it prints become the `cookie`, `gateway`, `gwcert` and `resolve` secrets. A prompt
to accept a certificate that failed verification ends the sign-in. Why it failed (no openconnect, the certificate, the
server's last message) is noted for `connect_vpn`. Before it connects an OpenConnect VPN, `connect_vpn` gives a profile
without them the flags 2 (not saved) for `cookie`, `gateway`, `gwcert` and `resolve`, as the plugin's editor does
(`Update`, with the secrets NetworkManager keeps): NetworkManager drops secrets from an agent for keys it takes to be
stored, which a profile made with `nmcli connection add` would lead to. Nothing is stored by Sway Things: the secrets go
back to NetworkManager, which keeps them as their flags say; those left to an agent are asked for again (no keyring).
Without an auth dialog (only the plugin's KDE part, or its GNOME package missing) the agent answers `NoSecrets` and
notes it, so `connect_vpn` reports which plugin's GNOME part is missing. Everything else (802.1X, WireGuard, whose keys
are in the connection) gets `NoSecrets`, so another agent can answer. `delegate` starts nm-connection-editor: the whole
editor, `--create --type=802-11-wireless`, or `--edit` with a connection's UUID, which is checked to be one before it
becomes an argument.

Sound needs none either: the sound server (PipeWire with WirePlumber, or PulseAudio) owns the defaults, volumes and
profiles and remembers them. `crates/core/src/sound/` speaks the PulseAudio protocol through libpulse, which
pipewire-pulse serves too. `Sound` runs libpulse's threaded main loop: every call into libpulse holds the loop's lock
(let go even if the binding panics over an operation libpulse refuses at once, which becomes an error), the callbacks
only send what they got into an `async-channel`, and the async methods await it, so the core stays executor-agnostic and
the GUI never blocks. A snapshot holds the server with its defaults, the outputs (sinks) and inputs (sources, without
the monitors of the outputs) with volume per channel, mute and ports, the cards with their profiles, and the streams of
the apps that play (sink inputs) and record (source outputs), our own level meter left out. `changes()` ticks on every
state change of the connection and every event of `subscribe`; reading gives a tick back only when it took one while
waiting for the connection, so reading never asks for the next read. Without a server, or after it went away (a restart
of pipewire-pulse), the context fails and the next snapshot connects again, at most once a second (`generation()` counts
the connections, so the page starts its level meter again on a new one); a call on a connection that isn't ready becomes
`Unavailable` (checked with the main loop's lock held, so the state can't change before the call); any other call
libpulse refuses makes the binding panic over a null operation, which is caught and becomes the context's error. That
relies on panics unwinding, as in every profile of the workspace: built with `panic = "abort"`, such a refusal would end
the app. `changes()` holds at most one tick, which is all a tick says, so a holder that never reads it (the CLI's
doctor) builds up no backlog. `NOFAIL` isn't used, since it only waits for the first connection. The actions set the
default output and input (the streams that follow the default move along by themselves, on PipeWire and on PulseAudio
since 15.0; moving them would pin them), volume (scaled on the loudest channel, so the balance stays), mute and port of
a device, volume and mute of a stream, and the profile of a card. `Meter` measures an input's level with a recording
stream that asks only for the peak (`PA_STREAM_PEAK_DETECT`, one float channel at 25 Hz), as pavucontrol does. It keeps
the input awake while it runs: on PipeWire, a stream with `PA_STREAM_DONT_INHIBIT_AUTO_SUSPEND` is passive and gets
nothing from an input no app records from. pipewire-pulse marks a peak-detect stream `stream.monitor = true`, and
WirePlumber's Bluetooth autoswitch (`bluetooth.autoswitch-to-headset-profile`) ignores such streams, so the meter never
switches a headset from A2DP to HSP/HFP (a headset in A2DP has no microphone; its input stays quiet). Privacy indicators
that list the streams recording from a microphone, such as waybar's privacy module, may show the microphone in use while
the meter runs, which is true: it reads the microphone, only while the page is on screen. The stream and its read
callback (on libpulse's thread) share an `Rc<RefCell<…>>`, sound only because every touch happens with the main loop's
lock held. The volume of alert sounds is left out: `libpulse-binding`'s stream-restore write hands libpulse a Rust array
where it expects C structs, and a fix would need unsafe code. The page shows volumes up to 100 %, or 150 % with
`app.sound_above_100`; `delegate` starts pwvucontrol, else pavucontrol, for the rest.

Printers are no `Backend` either: CUPS owns the queues, the default printer and the jobs, and keeps them.
`crates/core/src/printers/` talks IPP to the local CUPS. Neither the `ipp` crate (its client is built on reqwest and
tokio) nor libcups (FFI needs `unsafe`) fits, and the output of `lpstat` depends on the locale, so the module has an IPP
encoder and decoder of its own (`printers::ipp`, RFC 8010: groups, every value tag, collections nested at most 16 deep,
strings that aren't UTF-8 kept lossy, at most 100,000 values a message, never a panic, property-tested on arbitrary and
cut-off bytes) and the bit of HTTP/1.1 it needs (`printers::http`: one `POST` per connection with `Connection: close`,
answers with a length or chunked, read as the bytes come with the header taken apart once, at most 8 interim answers, a
body of at most 4 MiB (the answers of a CUPS with 20 printers are about 20 KiB), two different `Content-Length`s
refused, a timeout per request), on async-io like the rest of the core. `Cups::local` finds CUPS as libcups does:
`CUPS_SERVER`, the `ServerName` of `~/.cups/client.conf` or `/etc/cups/client.conf`, the domain socket
(`/run/cups/cups.sock`), else `localhost:631` (another port on `127.0.0.1` or `[::1]` as given); a server on another
host is an error the page shows, since Sway Things only manages this computer's CUPS and makes no network requests.
Without a server set, or with a loopback address at port 631, the socket or the port is chosen again for every request
(`Endpoint::Default`), as libcups does for each connection: systemd's `cups.socket` only makes the socket and removes it
when stopped, so a CUPS started while the page is open is found, over the socket that `PeerCred` needs. A snapshot is
three reads: `CUPS-Get-Printers` (name, description, location, model, device URI, state, reasons, message, accepting,
shared, temporary, type, `marker-*`), `CUPS-Get-Default` and `Get-Jobs` (not completed, of every user); the user's own
default comes from the `Default` line of `~/.cups/lpoptions`, which is only read, when the page comes on screen rather
than with every snapshot. Every string from CUPS is cleaned of control characters; reasons become words with their
severity (`media-empty-error` → _Out of paper_, `offline-report` → _Offline_). The actions are `Pause-Printer`,
`Resume-Printer` with `CUPS-Accept-Jobs`, `CUPS-Set-Default`, `CUPS-Delete-Printer`, `CUPS-Add-Modify-Printer` (a new
driverless printer with `ppd-name=everywhere`, which makes CUPS ask the printer for its attributes and build the driver;
or the description, location and sharing), `Cancel-Job` and `Print-Job` of a test page (CUPS's `testprint`, else
cups-filters' `default-testpage.pdf`, else a one-page PDF of our own; a document CUPS has no filter for moves on to the
next). `add` reads the queues first: a name taken since the page chose it gets a free one like it, since
`CUPS-Add-Modify-Printer` would change the queue that has it. CUPS 2.4.10 keeps the queue of a printer it couldn't add;
`add` removes it again, which only ever is the queue it made. Every request names the user (`requesting-user-name`,
without control characters or spaces). **Authorization:** when CUPS answers `401` over its domain socket and offers
`PeerCred`, the request goes again with `Authorization: PeerCred <user>`: CUPS checks the socket's peer credentials, so
members of its `SystemGroup` change printers without a password and owners cancel their jobs. When CUPS still refuses
(`401`, `403`, or IPP's `client-error-forbidden`, `-not-authenticated`, `-not-authorized`), a change goes to
cups-pk-helper (`org.opensuse.CupsPkHelper.Mechanism` on the system bus) with `ALLOW_INTERACTIVE_AUTH`, so the user's
polkit agent asks for an administrator's password: `PrinterSetEnabled`, `PrinterSetAcceptJobs`, `PrinterSetDefault`,
`PrinterDelete`, `PrinterAdd` (then enabled and accepting, as GNOME Settings does), `PrinterSetInfo`,
`PrinterSetLocation`, `PrinterSetShared` and `JobCancelPurge`. Without cups-pk-helper the error names it and the group
to join (`lpadmin`, `wheel` or `sys`, whichever `/etc/group` has); a refusal of polkit says that no agent may be
running. Sway Things never asks for, sends or keeps a password. **Discovery:** `Discovery` has its own connection to the
system bus, subscribed to Avahi's `ServiceBrowser` signals before it creates the browsers for `_ipps._tcp` and
`_ipp._tcp`, so none is missed on any Avahi version; it resolves only the TXT record of a new service
(`AVAHI_LOOKUP_NO_ADDRESS`; `ty` is the model, `note` the location, `rp` and the host name tell whether it is set up
already), several side by side, so one that doesn't answer within 5 s holds up none and shows by its name, merges the
announcements of one printer (both types, every interface and protocol; the secure one wins) and leaves out the queues
our own CUPS shares (announced from this computer with `printer-type`, or from this computer without an answer), which
would print to themselves. It keeps track of at most 200 printers, and leaves out services whose domain isn't a host
name (`[A-Za-z0-9.-]`). A printer found becomes a device URI of its service name (`ipps://Name._ipps._tcp.local/`),
which CUPS resolves through Avahi itself when it adds the printer; Sway Things never contacts a printer. Dropping the
`Discovery` closes its connection, and Avahi frees its browsers. **Live:** the page reads a snapshot every 2 s while it
is on screen, not while its window is suspended on a hidden workspace (three small reads over the socket, which CUPS
doesn't log at its default `AccessLogLevel`), at once when it comes on screen, and after every action; a CUPS
subscription would save the reads (Feature Catalog). `delegate` starts system-config-printer for printers that need a
driver; without it, the page opens CUPS's web interface at the port it reaches CUPS on.

## Screenshots, clipboard and recording

Screenshots, the clipboard history and recording need no `Backend` either: nothing is configured in another tool, the
keys run the command line (`crates/core/src/capture/`, `crates/cli/src/capture.rs`, `hook clipboard` in
`crates/cli/src/hook.rs`), which runs the tools and ends. It loads only the settings files and asks sway only what it
needs, so a key press is quick. The facts behind it are in the
[Sway Reference](Sway-Reference.md#screenshots-clipboard-and-recording).

- **Screenshots** (`capture::shot`): a region is selected with slurp, which gets the windows on screen (`GET_TREE`,
  `visible`) as boxes to click; the focused window is its `rect`, the focused display its name (`GET_OUTPUTS`). grim
  writes the picture to standard output in the format of the name's extension (`png` by default, `jpg`, `ppm`). The name
  comes from the pattern, filled in with the local time by `g_date_time_format` (`capture::name`); before the picture is
  taken, so a pattern that makes no name costs none. The file is created only if no file of its name exists
  (`create_new`), else `name (2).png` and so on (a thousand tries, then an error, never a name that wasn't checked), in
  the folder (`$XDG_PICTURES_DIR/Screenshots` from GLib's user directories, else `~/Pictures/Screenshots`), made when
  needed. `wl-copy --type <mime>` copies it, with no pipe to its output kept, since it forks and serves the clipboard.
  For _Copy and save_, saving and copying are tried each on its own: when only one works, the notification says which
  (_Screenshot saved, but not copied_) and why, and the command exits with 1; the picture is never lost. A canceled
  selection is `CaptureError::Canceled`: no picture, no message, exit 0.
- **Afterwards:** a notification (`capture::notify`, `org.freedesktop.Notifications` over zbus) with the picture
  (`image-path`), _Open_ (GIO's default app for the file) and _Edit_ (swappy `-f <file> -o <file>`, or satty
  `--filename <file> --output-filename <file>` with `%` doubled; a picture only copied goes to the editor's standard
  input, and only then is it kept in memory while the command waits). The body has `&`, `<` and `>` escaped, since
  servers may read it as markup. The command waits for a button or the notification to close, at most two minutes, only
  if the server shows buttons (`GetCapabilities`). Errors go to standard error and to a notification, since a key has no
  terminal.
- **Processes and locks** (`capture::process`): the watcher and a recorder are found in `/proc` by their name, the
  marker in their environment and the caller's sway session (`XDG_RUNTIME_DIR` and `WAYLAND_DISPLAY`); a caller without
  either finds none, so a command from a TTY or ssh never ends another session's processes. Paths of the arguments are
  kept byte for byte. A lock is `sway-things-<name>.lock` in `$XDG_RUNTIME_DIR` with a token of its holder; one older
  than ten seconds is taken over by renaming a file with a new token over it and reading it back, and only the holder
  whose token is in it removes it. These locks are the page's own, apart from the mirror's.
- **Recording** (`capture::record`): `sway-things record` holds the lock `record` from looking for a recording until a
  new one runs, so a second press meanwhile starts none (it says so). The file is made empty with `create_new` first, so
  no other file is written over, and wf-recorder gets `-y` (it would ask before writing over it); then
  `setsid -f wf-recorder` (else wl-screenrec) `-g <geometry>` or `-o <display>` `-f <file>` `[--audio]`, detached like
  `wl-mirror`, with `SWAY_THINGS_RECORDING=<notification id>` in its environment and its standard error in
  `$XDG_RUNTIME_DIR/sway-things-recording.log`; 0.7 s later it must still run, or the last line of the log is the error
  and the empty file goes. Only `.mp4` and `.mkv`: both recorders encode H.264 by default, which WebM can't hold. The
  notification that says it records goes after the server's usual time, so it isn't recorded for long, and takes the
  place of the last one about a recording (its id in `$XDG_RUNTIME_DIR/sway-things-recording.notification`). The next
  press finds the recorder, sends `SIGINT` (`kill -s INT`), waits up to ten seconds for it to end, so the file is
  complete, lets go of the lock and replaces the notification with where the file is and _Open_. The page looks for a
  recording every two seconds while it is on screen and shows _Stop_, which runs `sway-things record` without waiting
  for it.
- **Clipboard history** (`capture::clipboard`): `sync_with` has exactly the watcher run that the settings ask for:
  `setsid -f wl-paste --watch cliphist -max-items <n> store` with `SWAY_THINGS_CLIPBOARD=1`, or none; it ends the marked
  ones that differ (also when the history was switched off by hand in `settings.toml`), so running it twice changes
  nothing. It takes the lock `clipboard`, waiting up to three seconds, and reads the settings only once it holds it, so
  of the hook and the app running at once the second acts on what is set by then. A `wl-paste --watch cliphist store` of
  the user's own in the session means none of Sway Things runs; `hook clipboard` says so on stderr, and outside a
  session (no `XDG_RUNTIME_DIR` or `WAYLAND_DISPLAY`) it says why it does nothing. `hook clipboard` in `80-hooks.conf`
  runs it at every start and reload of sway while the history is on; the app runs it when the switch or the number of
  entries changes and stood still for 0.8 s, and the command line after `set` or `reset` of either. `pick` runs
  `cliphist list`, hands it to the launcher's dmenu mode (exit status 1 with nothing on stderr is a cancel; any other
  failure is reported), the line picked to `cliphist decode`, and the entry to `wl-copy`, with `--type` for a PNG, JPEG,
  GIF, BMP or WebP picture found by its first bytes; an entry without data is reported, not copied. `skips_passwords`
  compares the versions of wl-paste and cliphist. clipman isn't offered: it keeps only text.
- **Programs** (`capture::tools`): each with its package; `missing_for` says which part that is on misses what, for
  `doctor` and the page.

## GSettings

Some apps don't ask sway: GTK apps take their cursor theme and size from `org.gnome.desktop.interface`, and GTK,
libadwaita and the browsers their theme, icons, fonts, text size and light or dark style. A setting with a GSettings key
(`Setting::gsettings`) is copied there by `crates/core/src/gsettings.rs`, through GIO, so the schema and the backend
(dconf) are the desktop's. The schema is looked up first, so a missing one is an error, not an abort. A text or a choice
goes to a string key (an enum key takes the choice's id), a whole number to an integer key, a decimal to a double key.

- **Only in GSettings:** the settings of Apps Look & Feel (`look.*`) have no sway directive. Without a stored value,
  their row and `sway-things get` show what GSettings has now (`SettingView::with_desktop`, source _GSettings_), so a
  theme picked with another tool is what the page says; the app follows changes of the schema while it runs. A change of
  the cursor theme shows sway's value, not GTK's, since sway draws the cursor. GTK 4, libadwaita and the browsers learn
  the style only from the Settings portal, so the page tells to install xdg-desktop-portal-gtk when no portal reports
  `org.freedesktop.appearance` `color-scheme`, and when the schema itself is missing.

- **After each change** (set, reset, "Let my config win", a settings file changed on disk) the `Applier` copies the
  settings of the changed directive: a stored value is written, and once nothing is stored the key is reset to its
  schema default. A key that can't be written (no schema, locked) is reported with the change (`not_copied`), which is
  made anyway; the CLI prints a warning.
- **At login and each reload** `80-hooks.conf` runs `sway-things hook gsettings` while such a value is stored. It writes
  the stored values again and leaves the other keys alone, so a `settings.toml` from another machine reaches dconf too,
  and a cursor set with another tool stays while Sway Things stores none.
- **A stored value changed elsewhere** (nwg-look, a day/night script): the row keeps showing the stored value and says
  what apps use now (`SettingView::changed_elsewhere`); Sway Things writes its own again when sway starts or reloads. A
  setting another one depends on is read the same way, so subpixel smoothing set elsewhere unlocks the subpixel order.
- **Nothing that keeps values:** without dconf, GIO keeps values in memory, so a write works and is lost. Such a write
  is reported like a missing schema (`GSettingsError::NotKept`), and the page says to install dconf.
- **`doctor`** checks every stored value with a key without writing it, against the key's type and range (an enum's
  values), and warns about one GSettings can't take (no schema or key, locked, out of range, nothing keeps it), with a
  hint for each: apps that read it there don't get it.
- **Tests** never touch the user's dconf: `Applier::with_session` and `without_session` copy nothing until
  `set_gsettings` names a store, and the tests use `MemoryGSettings`. Only `Applier::connect` (the app and the CLI) uses
  GIO.

Planned with the trait: if an unknown tool or a custom script is in charge (e.g. an `exec ~/.local/bin/my-wallpaper.sh`
line), `detect()` reports **managed externally by file:line**. The page explains this and offers a handover. It never
fights the script.

For wallpapers, `wallpaper::starters` lists the `exec` and `exec_always` lines of the user's config that start a known
wallpaper tool (swaybg, swww, wpaperd, hyprpaper, mpvpaper, waypaper, azote, …) or a program whose name contains
"wallpaper", also inside a shell command. Such a tool draws a layer of its own over or under the background of
`output … bg`, so the Wallpaper page names them, one notice per program with its lines, and a button that opens the
first in the editor. A line that runs `sway-things` itself isn't one.

## Wallpaper collections

`crates/core/src/wallpaper/` holds the collections: named sets of pictures and folders (a folder adds the pictures in
it, read each time) with when the picture changes (`login`, `daily`, `interval` with the minutes) and the order
(`shuffle` never shows the same picture twice in a row; `in_order`). An output shows one when `wallpaper.collection`
names it.

- **The link:** the `bg` line of such an output names a symbolic link in `$XDG_STATE_HOME/sway-things/wallpaper/`
  (`all`, or `output-<identifier>` with the identifier escaped like `systemd-escape` does, since it comes from the
  screen and may hold anything; `Paths::wallpaper_link`), which points at the current picture. The `bg` setting renders
  the link through `Values::wallpaper_picture`, which `State` answers from its `Paths`, so the registry reads neither
  the environment nor the disk. The generated config stays the same while the pictures change, and sway shows the
  current one at login and after a reload. The link is replaced atomically, and its own modification time is the time of
  the last change. Before a `bg` line naming it is applied or written, `prepare` makes the links that are missing or
  point outside the collection; a collection without pictures gets a color line.
- **Changing:** `wallpaper::next` moves the link on and sends the output's `bg` command again; for every output it sends
  the commands of the outputs with a background of their own after it, since `output *` at runtime replaces theirs.
- **No daemon:** while any output shows a collection, `20-output.conf` ends with one `exec sway-things hook wallpaper`,
  which names no output, so no identifier of a screen reaches the shell that runs the line. For each output with a
  collection, it changes the picture if its time has come (at every login for `login`, else when the day or interval has
  passed since the last change) and starts a transient systemd user timer, `sway-things-wallpaper[-<identifier>]`
  (escaped like the link; `systemd-run --user`, no unit file), that runs `sway-things wallpaper next --due` (with
  `--device <identifier>` as an argument of its own for one output) with the session's `SWAYSOCK`: hourly for `daily` (a
  day counts from the last change, which needs no time zone), else at the interval. The app and `set`/`reset` start the
  timers again after a change of a collection or of the collection an output shows. A timer whose session ended stops
  itself. Without a systemd user manager, pictures change at login only, and the app offers only that.

## Apps and autostart

`crates/core/src/apps/` serves the Apps & Autostart page. The facts behind it are in the
[Sway Reference](Sway-Reference.md#apps-and-autostart).

- **Desktop entries** (`entry`): the `[Desktop Entry]` group of a `.desktop` file, read no larger than 256 KiB; strings
  with their escapes (`\s`, `\n`, `\t`, `\r`, `\\`), lists split at `;` (`\;` is part of an item), booleans, localized
  strings looked up for each language as `ll_CC@mod`, `ll_CC`, `ll@mod`, `ll`, then the key without a locale. `Exec` is
  split into arguments as the spec quotes it (double quotes with `\"`, `` \` ``, `\$`, `\\`; single quotes and a
  backslash outside quotes as GLib and systemd read them) and its field codes are filled in for a start without files:
  `%f %F %u %U` and the deprecated codes go, `%i` becomes `--icon <Icon>`, `%c` the name, `%k` the file, `%%` a `%`.
  Names, icons and command lines are someone else's data: they become label text, an icon name or image file, or an
  argument array, never a shell string.
- **Installed apps** (`installed`): the entries below `applications/` of `$XDG_DATA_HOME` and `$XDG_DATA_DIRS`, each
  under its desktop file ID (the path with `/` as `-`). The first directory with an ID wins; `Hidden=true` there hides
  the ID, a `TryExec` program that isn't installed too. `OnlyShowIn`/`NotShowIn` decide by the first desktop of
  `XDG_CURRENT_DESKTOP` either names, as `systemd-xdg-autostart-condition` does.
- **Terminal and launcher** (`launchers`, settings `apps.terminal`, `apps.launcher` and their `…_keys`): a command line
  each, empty while the user's own binding is kept. The page offers the known terminals and launchers in `PATH` (foot,
  Alacritty, kitty, …; fuzzel, `wofi --show drun`, `rofi -show drun`, …) and other `TerminalEmulator` entries by their
  program. The bindings go to `50-bindings.conf` (see [Key bindings](Reading-Sway.md#key-bindings)) and change live like
  the page's other bindings. The user's `$term` and `$menu` can't be changed for their lines, since sway replaces a
  variable when it reads a line; the page shows them and what the user's config binds to the keys.
- **Default apps** (`mime`, settings `apps.default.*`): web browser, file manager, mail, image viewer, PDF viewer, video
  and music player, text editor, terminal, each with its MIME types (`x-scheme-handler/https`, `inode/directory`,
  `application/pdf`, …). Candidates are the installed apps whose `MimeType` has the kind's main type (for the terminal
  also `TerminalEmulator` entries), `NoDisplay` ones included. `sway-mimeapps.list` names the chosen app in
  `[Default Applications]` for the types of its kind that it opens (all of them for one that isn't installed);
  `sway-xdg-terminals.list` names the terminal for `xdg-terminal-exec`, which GLib 2.76+ and the
  [editor](Reading-Sway.md#editor) start `Terminal=true` apps with. What opens a type now follows the spec: the lists in
  order (`$XDG_CONFIG_HOME`, `$XDG_CONFIG_DIRS`, then `applications/` of the data directories; in each the
  `<desktop>-mimeapps.list` of every desktop of `XDG_CURRENT_DESKTOP`, lowercase, then `mimeapps.list`), the first
  installed ID of `[Default Applications]` winning; without one, the first installed app of `[Added Associations]`, else
  the first that opens the type. A row says whether Sway Things' list wins, and which list does instead.
- **Autostart entries** (`autostart`, setting `autostart.entries`): `exec <command line>` or `app <desktop file ID>`,
  switched off with `#` and a space in front. `70-autostart.conf` has an `exec` line for each one switched on, the
  command line as one quoted argument (`SwayCmd::exec`): sway defers `exec` while it reads the config and runs the line
  through `execute_command` later, which replaces `$$` once and hands a single quoted argument to `sh -c` without its
  quotes, so the shell gets exactly the command line (a test runs both renderings through the port of sway's parser). An
  app runs `sway-things hook launch <ID>`, which reads its desktop entry when it starts (an update of the app changes
  nothing here) and starts its arguments with `Path` as the working directory, in a terminal for `Terminal=true`
  (`xdg-terminal-exec` first, so the terminal chosen on the page counts only where it is installed; else a known
  terminal whose option takes the command as separate arguments: `-e` of Alacritty, Ghostty, Konsole and xterm, `--` of
  Console and GNOME Terminal, `-x` of Xfce Terminal; not Tilix, whose `-e` takes one string). An app with
  `DBusActivatable=true` and no `Exec` is activated over the session bus (`org.freedesktop.Application.Activate` on the
  ID's bus name and object path). A hidden app or one whose `TryExec` program is missing is reported as such. The ID
  consists only of letters, digits, `-`, `_` and `.` (no `/`, so nothing leads out of the directories), and an entry's
  command line can't start with `-`: an option of sway's `exec` (`--no-startup-id`) would reach the shell as its
  command. _Start Now_ and _Start Them Now_ save what isn't saved yet (the systemd unit, an entry just changed), then
  send the same `exec` over IPC.
- **The user's `exec` lines** (`hand_written`): every top-level `exec` and `exec_always` of the user's config with file
  and line, and the runner of XDG autostart entries one starts (`dex` with `-a`/`--autostart`, or a line naming
  `xdg-desktop-autostart.target` or Sway Things' unit; a session target such as `sway-session.target` counts only
  through `xdg-desktop-autostart.target` being active).
- **XDG autostart entries** (`xdg_entries`): `$XDG_CONFIG_HOME/autostart`, then `autostart/` of `$XDG_CONFIG_DIRS`; the
  first file of a name counts, as for the runners. Each says whether it starts in this session: `Hidden=true`,
  `OnlyShowIn`/`NotShowIn`, a missing `TryExec` program, an `Exec` program that isn't installed, no `Exec`.
  `X-GNOME-Autostart-enabled=false` is only read by GNOME's session; dex and systemd start such an entry, and the page
  says so. A file of the user's is theirs (Open in Editor); one that starts with the header of the generated files is
  Sway Things' override and shows the system entry it switches off.
- **Switching off a system entry** (`autostart.hidden`): Sway Things' own `~/.config/autostart/<ID>.desktop` with
  `Hidden=true`, written only where no file of that name exists; switched on again, the file is deleted.
- **The runner** (`autostart.runner`: `none`, `systemd`, `dex`): sway starts no XDG autostart entries itself.
  - `dex` becomes `exec dex --autostart --environment sway`. Its apps inherit all of sway's environment.
  - systemd's `xdg-desktop-autostart.target` refuses a manual start (`RefuseManualStart=yes`), so `systemd` has Sway
    Things write its own user unit `sway-things-autostart.target` (`Wants=` and `Before=` the target) and
    `exec sway-things hook autostart`. At each start of sway the hook hands the session's variables to the user manager
    (`systemctl --user import-environment`: `WAYLAND_DISPLAY`, `DISPLAY`, `SWAYSOCK`, `I3SOCK`, `XDG_CURRENT_DESKTOP`,
    `XDG_SESSION_TYPE`, `XCURSOR_THEME`, `XCURSOR_SIZE`, those that are set, as sway's own `50-systemd-user.conf` and
    sway-systemd do), runs `daemon-reload` (the generator reads the autostart entries again, and a unit written since is
    known), stops Sway Things' unit and `xdg-desktop-autostart.target` and waits, then starts the unit. The stop is what
    makes it robust: the user manager can outlive a sway session (lingering, a login on a TTY or over ssh, a second
    session), and starting a target that is still active can't be relied on to start anything anew. It ends no app,
    since `Wants=` carries no stop and the entries' services aren't part of the targets; an entry whose service still
    runs isn't started twice. The alternative of binding the unit to the session needs a unit that ends with it, which
    nothing provides without a session manager.
  - Apps started by systemd get the user manager's environment plus the variables imported, not sway's whole
    environment: `PATH` stays the user manager's (a login shell's `PATH` would change every service of the user), and
    secrets such as `SSH_AUTH_SOCK` aren't copied. The user manager is shared by all sessions of the user: a second or
    nested sway that starts the hook points the variables, and so apps started later through systemd, at its own
    display.
  - The page and `doctor` tell what runs the entries now: the target active in this session (a systemd session such as
    uwsm or sway-systemd), a line of the user's that runs `dex` with `-a`/`--autostart` as an argument or starts
    `xdg-desktop-autostart.target` or Sway Things' unit, or Sway Things. With none, the offer is systemd if the target
    is known (`systemctl --user show`), else dex if it is installed. With two (dex next to dex or systemd; whatever
    starts systemd's target counts once, since starting it again does nothing), a warning that each entry may start
    twice. While the target runs only because Sway Things' unit started it before the runner was changed, the page says
    the change applies at the next login instead.
- **Files of other programs** (`files`): `sway-mimeapps.list`, `sway-xdg-terminals.list`, the overrides and the systemd
  unit start with the header of the generated files and are recorded in `manifest.toml` with their hash, like the sway
  files. `Applier::persist` writes them after the sway files, for the user's directories (`Applier::set_apps_dirs`;
  appliers made for tests write none): a missing one is created without replacing anything (`atomic::create_new`:
  written next to its place, then linked there, which fails if a file appeared meanwhile), one of Sway Things as it
  wrote it is replaced (through a symbolic link, like the sway files), and one no longer needed is deleted, only then (a
  link to it stays); someone else's file, a dangling link, or a file of Sway Things changed since, is left alone and
  reported (`Persisted::apps`). For the default apps only the chosen apps are looked up, not every installed one, so a
  save stays cheap.
