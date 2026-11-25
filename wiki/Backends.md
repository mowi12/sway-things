# Backends

> Part of the [architecture](Architecture.md). The services and tools Sway Things drives besides sway: BlueZ,
> NetworkManager, the sound server, CUPS, SANE, systemd-timedated, the notification daemons and GSettings, the tools of
> Screenshots & Clipboard, logind, ddcutil and the night-light tools of Brightness & Night Light, the desktop entries,
> default apps and autostart runners of Apps & Autostart, and the planned backends for the companion tools.

The plan (not built yet) is a backend trait for each area that has competing tools. Today the companion tools Sway
Things drives are kanshi, in `crates/core/src/outputs/kanshi.rs` ([Outputs](Outputs.md)), and the notification daemons
([Notifications](#notifications)), without a trait yet; Bluetooth, the network, sound and printers are services with no
competing stack and use no trait (below).

```rust
// planned
trait Backend {
    fn detect(&self) -> Availability;   // installed? running? version? managed externally by file:line?
    fn read(&self) -> Result<Values>;
    fn apply(&self, change: &Change) -> Result<()>;
    fn generate(&self, state: &State) -> Vec<GeneratedFile>;
}
```

| Area | Backends (reference first); only display profiles, audio, Bluetooth, the network, printers, notifications and the system clock are built |
| --- | --- |
| Display profiles | kanshi, per combination of displays; without kanshi: static `output` lines in `20-output.conf` (no profiles); shikane later |
| Bar | waybar, swaybar (`bar {}` block) |
| Idle / lock | swayidle + swaylock |
| Notifications | mako, swaync, dunst (see [Notifications](#notifications)) |
| Night light | wlsunset, gammastep (see [below](#brightness-and-night-light)) |
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
| Scanners | SANE's `scanimage` (subprocess); Avahi (D-Bus) |
| Power | power-profiles-daemon / tuned-ppd, UPower (D-Bus) |
| Brightness | logind `Session.SetBrightness` (no root needed); ddcutil for external monitors (see [below](#brightness-and-night-light)) |
| Time / locale | systemd-timedated, systemd-localed (D-Bus, polkit) |

Bluetooth needs no `Backend`: it is runtime state that BlueZ owns and keeps itself, with nothing to store or generate,
and no competing stack. `crates/core/src/bluetooth/` has a `Bluez` handle on the system bus (zbus, async-io): one
`GetManagedObjects` call gives a snapshot of the adapters and devices, the actions are D-Bus calls, and a stream of
`PropertiesChanged`, `InterfacesAdded`/`Removed` and `NameOwnerChanged` signals (without the noise of signal strength)
tells the page to read a new snapshot. Pairing registers our agent (`org.bluez.Agent1`, capability `KeyboardDisplay`)
before each `Pair`, without `RequestDefaultAgent`, so BlueZ asks it only about our pairings; it answers only calls from
the owner of `org.bluez` and hands each question to the page as a prompt. _Cancel_ on a row sends `Disconnect` during a
`Connect` and `CancelPairing` during a `Pair`, and drops the result of the canceled call. Turning the radio off cancels
them the same way before it sets `Powered`, since BlueZ may hold the change while a connection attempt runs. The page
runs `StartDiscovery` while it is on screen and the radio is on, or with `app.bluetooth_search = "manual"` only for 30 s
after _Search_. `bluetooth::rfkill` reads `/dev/rfkill` for the soft and hard block and lifts a soft block by writing
one `RFKILL_OP_CHANGE_ALL` event. A page's "Advanced…" button starts a specialist tool from `delegate`
(blueman-manager), found in `PATH`.

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
reason, or for a VPN the reason of its `StateChanged` signal; `connect_vpn` subscribes to that signal before it
activates the VPN, because NetworkManager asks our agent for the secrets at once and a VPN that fails then can end
before `ActivateConnection` returns. The change stream takes every signal of NetworkManager under `/org/freedesktop` and
`NameOwnerChanged`, without traffic counters, `LastSeen`/`LastScan` and NetworkManager's old duplicate
`PropertiesChanged` signals. Our secret agent (`org.freedesktop.NetworkManager.SecretAgent` at its fixed path,
registered as `io.github.mowi12.SwayThings` with the capability `VPN_HINTS`) is registered while the page is on screen
and until a connection it started has finished, and again when the owner of `org.freedesktop.NetworkManager` changes
(NetworkManager forgets agents when it restarts). It answers only NetworkManager: for WPA and WPA3 Personal passwords
when it may ask the user, and for the `vpn` secrets of a VPN connection through its plugin's auth dialog
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
driverless printer with `ppd-name=everywhere`, which makes CUPS ask the printer for its attributes and build the driver,
and `printer-is-shared=false`, since CUPS shares a new queue while `DefaultShared` is on, as it is by default; or the
description, location and sharing), `Cancel-Job` and `Print-Job` of a test page (CUPS's `testprint`, else cups-filters'
`default-testpage.pdf`, else a one-page PDF of our own; a document CUPS has no filter for moves on to the next). `add`
reads the queues first: a name taken since the page chose it gets a free one like it, since `CUPS-Add-Modify-Printer`
would change the queue that has it. CUPS 2.4.10 keeps the queue of a printer it couldn't add; `add` removes it again,
which only ever is the queue it made. Every request names the user (`requesting-user-name`, without control characters
or spaces). **Authorization:** when CUPS answers `401` over its domain socket and offers `PeerCred`, the request goes
again with `Authorization: PeerCred <user>`: CUPS checks the socket's peer credentials, so members of its `SystemGroup`
change printers without a password and owners cancel their jobs. When CUPS still refuses (`401`, `403`, or IPP's
`client-error-forbidden`, `-not-authenticated`, `-not-authorized`), a change goes to cups-pk-helper
(`org.opensuse.CupsPkHelper.Mechanism` on the system bus) with `ALLOW_INTERACTIVE_AUTH`, so the user's polkit agent asks
for an administrator's password: `PrinterSetEnabled`, `PrinterSetAcceptJobs`, `PrinterSetDefault`, `PrinterDelete`,
`PrinterAdd` (then enabled and accepting, as GNOME Settings does), `PrinterSetInfo`, `PrinterSetLocation`,
`PrinterSetShared` and `JobCancelPurge`. Without cups-pk-helper the error names it and the group to join (`lpadmin`,
`wheel` or `sys`, whichever `/etc/group` has); a refusal of polkit says that no agent may be running. Sway Things never
asks for, sends or keeps a password. **Discovery:** `Discovery` has its own connection to the system bus, subscribed to
Avahi's `ServiceBrowser` signals before it creates the browsers for `_ipps._tcp` and `_ipp._tcp`, so none is missed on
any Avahi version; it resolves only the TXT record of a new service (`AVAHI_LOOKUP_NO_ADDRESS`; `ty` is the model,
`note` the location, `rp` and the host name tell whether it is set up already), several side by side, so one that
doesn't answer within 5 s holds up none and shows by its name, merges the announcements of one printer (both types,
every interface and protocol; the secure one wins) and leaves out the queues our own CUPS shares (announced from this
computer with `printer-type`, or from this computer without an answer), which would print to themselves. It keeps track
of at most 200 printers, and leaves out services whose domain isn't a host name (`[A-Za-z0-9.-]`). A printer found
becomes a device URI of its service name (`ipps://Name._ipps._tcp.local/`), which CUPS resolves through Avahi itself
when it adds the printer; Sway Things never contacts a printer. Dropping the `Discovery` closes its connection, and
Avahi frees its browsers. **Live:** the page reads a snapshot every 2 s while it is on screen, not while its window is
suspended on a hidden workspace (three small reads over the socket, which CUPS doesn't log at its default
`AccessLogLevel`), at once when it comes on screen, and after every action; a CUPS subscription would save the reads
(Feature Catalog). `delegate` starts system-config-printer for printers that need a driver; without it, the page opens
CUPS's web interface at the port it reaches CUPS on.

Scanners aren't a `Backend` either: SANE owns them. `crates/core/src/scanners/` runs SANE's `scanimage` (sane-utils,
sane-backends), always as an argument array, never through a shell, with the device name as one argument
(`--device-name=NAME`); libsane through FFI would need `unsafe`. **Listing:** `scanimage -f` with a format of our own
(`%d`, `%v`, `%m`, `%t` separated by U+001F, which no name holds) gives a line per device, read by `scanners::list`
(which reads `-L`'s lines too); webcams of the v4l backend are left out, a device name with control characters is left
out (it couldn't be passed back), the rest is cleaned of them. How a scanner is connected comes from its device name:
`airscan:e…`/`airscan:w…` and `escl:` are driverless on the network (eSCL, WSD), or on USB through ipp-usb when their
address is the loopback one; `net:` is another computer's saned; a backend's name with `usb` in it is USB. Listing runs
off the main loop with a timeout of 45 s (sane-airscan waits a few seconds for answers from the network) and only when
the page comes on screen or on _Search Again_, since SANE has no change events. **Avahi:** `Discovery::scanners` browses
`_uscans._tcp` and `_uscan._tcp` like the printers' discovery (above), but keeps the services this computer announces,
which are USB devices that ipp-usb serves; the page listens for 3 s beside the listing. `scanners::sees` tells whether
SANE lists a scanner Avahi finds: sane-airscan names its devices after the service and gives that name as the model,
SANE's escl backend gives the model the scanner announces (`ty`). **What is missing** (`scanners::look`, read from files
only, below a root the tests replace): scanimage in `PATH`; sane-airscan as `libsane-airscan.so*` in `/usr/lib/sane`,
`/usr/lib64/sane` or a multiarch directory; ipp-usb installed (`PATH`, `/usr/sbin`) and running (`/proc/*/comm`); the
USB devices plugged in (`/sys/bus/usb/devices`), those with an interface of class 7, subclass 1, protocol 4 (IPP over
USB), and those libsane's udev hwdb or rules mark (`usb:vXXXXpYYYY*`, `ATTRS{idVendor}`/`ATTRS{idProduct}`), whose
device file in `/dev/bus/usb` the user must be able to read and write (`access(2)`), with its group from `/etc/group`;
and the distribution's family from `os-release`, for the package names. The hints and `sway-things doctor`'s _Scanners_
section come from that ([Sway Reference](Sway-Reference.md#sane-scanimage-and-the-driverless-scanners)). **Options:**
`scanimage --device-name=NAME --all-options` is read by `scanners::options`: the sources (glass, feeder, both sides),
the modes (color, gray, black and white, by the names the backend gives them: `Lineart`, `Binary`, …), the resolutions
(a list as it is, a range as the common values within it) and the area from `-x` and `-y` in mm, which gives the paper
sizes that fit (A4, Letter, Legal, A5, A6, 1.5 mm forgiven; Letter first in the Americas' locales). Only what the
scanner offers is offered, with its own values. **Scanning:**
`scanimage --device-name=NAME --format=F --progress --batch=DIR/page-%04d.EXT` with `--batch-count=1` for one page, or
without for every sheet of the feeder, then the device's options as separate arguments (`--source`, `--mode`,
`--resolution`, `-l 0 -t 0 -x -y`). The pages go into a directory of ours,
`$XDG_CACHE_HOME/sway-things/scans/scan-PID-TIME-N` (mode 0700, removed with the dialog; ones older than a day that a
crash left are removed); `--batch` works on every SANE version, unlike `--output-file` (1.0.28). The progress comes from
standard error (`Progress: N%`, `Scanning page N`); the rest of it, at most 16 KiB, is the error's details. A cancel
sends `SIGTERM`, which makes scanimage cancel the scan with the scanner, and `SIGKILL` 5 s later if it still runs; the
process isn't waited for until then, so its id can't be reused. scanimage that said `Batch terminated` (the end of every
batch, also after an error) gets 10 s to end, then it is killed: it can hang for good in `sane_exit` with its pages
written, and its errors are then read from its output alone. Exit statuses and messages become words (`Problem`): busy,
cover open, paper jammed, feeder empty (the normal end of a feeder scan once a page is in), no access, gone. **Saving:**
`scanners::save` writes the one file the user picked in the file chooser: the page as scanimage wrote it (PNG, JPEG,
TIFF), or a PDF of all pages from `scanners::pdf`, our own writer: JPEG pages go in as they are (`/DCTDecode`, the size
from the frame header), black and white pages come as PNM bitmaps and go in run-length encoded (`/RunLengthDecode`,
`/Decode [1 0]`), each page as large as the paper at the resolution it was scanned at. So PDF needs neither img2pdf nor
ImageMagick nor a scanimage with `--format=pdf` (1.1.1), only JPEG support, and pages of the glass and the feeder go
into one PDF. The file is written next to its place and renamed into it (`atomic::write_bytes`), so a failure never
leaves half a file. `delegate::scan_app` finds Document Scanner (`simple-scan`), Skanpage or XSane for what scanning
here doesn't do.

The system clock isn't a `Backend` either: systemd-timedated owns the time zone, automatic time and the clock.
`crates/core/src/region/` has a `Timedate` handle on the system bus (zbus, async-io). `GetAll` on
`org.freedesktop.timedate1` gives the `Clock` (`Timezone`, `NTP`, `CanNTP`, `NTPSynchronized`), `ListTimezones` the
zones for the dialog; the changes are `SetTimezone`, `SetNTP` and `SetTime` (microseconds since the epoch, not
relative), each with `interactive` true and the message flag that allows interactive authorization, so the user's polkit
agent asks for an administrator's password (timedated's actions are `auth_admin_keep`). A refusal (`AccessDenied`,
`InteractiveAuthorizationRequired`, a polkit error) becomes `Error::Denied`, and `region::Change::command` gives the
`timedatectl` command that does the same in a terminal, where polkit asks through `pkttyagent`; it is only shown, with
_Copy_, never run. A name nobody owns (no systemd) is `Error::Unavailable`. The change stream follows
`PropertiesChanged` on timedated's object only: timedated quits when idle and the bus starts it again on the next call,
which changes nothing. `NTPSynchronized` isn't reported as a change, so the page reads the clock every 5 s too.
`region::zones` reads the system's tzdata through jiff (`/usr/share/zoneinfo`, or `$TZDIR`; no bundled copy) for the
abbreviation, offset and local time of a zone, and turns the date and time picked in the clock's dialog into the moment
in the system's zone (a time skipped when the clocks go forward counts as the time after the gap).

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

## Brightness and night light

Neither needs a `Backend`: the brightness is live state nothing stores, and the night light is a tool started with an
argument list. The facts behind it are in the [Sway Reference](Sway-Reference.md#brightness-and-night-light).

- **Backlights** (`brightness`): `devices` reads `/sys/class/backlight` and the keyboard lights of `/sys/class/leds`
  (`*::kbd_backlight`), the screens first by preference (firmware, platform, raw), as systemd and GNOME prefer them.
  `Logind` sets one through `Session.SetBrightness` on `/org/freedesktop/login1/session/auto` over the system bus, so
  neither root nor brightnessctl is needed. A backlight never goes below 1 % (`Device::min_raw`, at least one raw step),
  so the panel can't go black; the keyboard light goes down to off. Nothing is stored: the kernel or `systemd-backlight`
  brings the backlight back at boot. `sway-things brightness` uses the same code.
- **External monitors** (`brightness::ddc`): `ddcutil detect --brief` lists the displays that answer over DDC/CI,
  without built-in panels (eDP, LVDS, DSI); each is addressed by its I²C bus (`--bus`), which stays while it is
  connected. `getvcp 10` and `setvcp 10` read and set the brightness (VCP feature `0x10`). Each call takes 50 to 500 ms,
  so the page runs them off the main loop and sends a value only once the slider stood still for 0.5 s, one at a time.
  `ddc::access` opens the `/dev/i2c-*` for reading and writing (which sends nothing) to tell missing rights from a
  missing `i2c-dev` module.
- **Night light settings** (`night_light.*`, `registry::night_light`): the switch, the tool (`wlsunset` by default, or
  `gammastep`), the schedule (`sunset`, `times`, `always`), the location (`timezone` or `custom` with latitude and
  longitude), the times (`from`, `to`, `HH:MM`) and the temperatures (night 4000 K, day 6500 K, both 1000–6500 K). They
  have no sway directive; while the switch is on, `80-hooks.conf` runs `sway-things hook night-light`.
- **The plan** (`night_light::plan`): wlsunset gets `-l`/`-L` from sunset to sunrise, or `-S <to> -s <from> -d 1800`
  with fixed times, and `-t <night> -T <day>`; it wants the day warmer than the night, so an equal or warmer day becomes
  `-t <night − 1> -T <night>`. _Always on_ is that pair at any times. gammastep gets `-c` with our
  `$XDG_CONFIG_HOME/sway-things/gammastep/config.ini` (header first; a file without it is someone else's and never
  written), with `adjustment-method=wayland`, the temperatures and either `location-provider=manual` with `[manual]` or
  `dawn-time`/`dusk-time` fading over half an hour before the night ends and after it starts, kept within the day. With
  fixed times the night must span midnight (`to` earlier than `from`), as both tools want it.
- **The location** (`night_light::location`): the time zone (`TZ`, else where `/etc/localtime` points into
  `/usr/share/zoneinfo/`) and its main city's coordinates from `zone1970.tab`, else `zone.tab`. Nothing asks a service
  where the computer is; a zone without coordinates (UTC) asks for a location by hand or fixed times.
- **Starting it** (`night_light::sync`): like the clipboard history's watcher, under the session's lock `night-light`.
  The tool runs detached (`setsid -f`) with `SWAY_THINGS_NIGHT_LIGHT=<fingerprint>` in its environment, a hash of the
  tool, its arguments and the config; one whose fingerprint matches the settings is kept, the others Sway Things started
  in this session are ended, and waited for up to three seconds (gammastep fades back while it holds the displays), then
  killed. The new one's standard error goes to `$XDG_RUNTIME_DIR/sway-things-night-light.log`; 0.5 s later it must still
  run, else the last line of the log is the reason. A night light of the user's own is never ended, and none is started
  while one runs (wlsunset, gammastep, gammastep-indicator, redshift, wl-gammarelay, sunsetr, found in `/proc` without
  the marker) or an `exec` line of the user's config starts one (`night_light::own_lines`). The app runs it 0.8 s after
  the settings stood still, off the main loop.

## Notifications

The notification daemon is no `Backend` either: `crates/core/src/notifications/` asks the one that runs and writes files
for it, each in its own format.

- **Which daemon runs:** `Notifications` (zbus, on the session bus) asks the owner of `org.freedesktop.Notifications`
  for `GetServerInformation` (mako says `mako`, swaync `SwayNotificationCenter`, dunst `dunst`) and the bus for its
  process. Every call goes to that unique name, never to a well-known one: a call to a name nobody owns makes the bus
  start whichever daemon has a D-Bus service file for it, and mako and swaync both have one for
  `org.freedesktop.Notifications`. So asking never starts a daemon.
- **What starts it** (`starts`): the `exec` lines of the user's sway config where a daemon is the program run (each
  command of the line, split at `;`, `&` and `|`, `sh -c` taken apart, variables and `exec`/`env` before it skipped: not
  `pkill mako`, `systemctl restart mako` or `swaync-client`; `killall swaync; swaync` starts one), our line in
  `80-hooks.conf`, a systemd unit (the process's control group ends in `….service`), else D-Bus activation or another
  program. The config a running daemon was given with `-c` comes from its `/proc/<pid>/cmdline`. Two daemons started
  from the config are reported: only the first to take the name shows notifications (swaync and mako end when they don't
  get it). A daemon the bus starts on demand (a D-Bus service file for the name, `starts::activatable`) counts as one.
- **Do Not Disturb:** mako: its modes (`ListModes`/`SetModes` on `fr.emersion.Mako`, what `makoctl mode -a/-r` calls),
  with `[mode=do-not-disturb] invisible=1` in our file; swaync: `GetDnd`/`SetDnd`; dunst: the property `paused` of
  `org.dunstproject.cmd0`. The page follows changes made elsewhere through `PropertiesChanged` (mako ≥ 1.10, dunst),
  swaync's `SubscribeV2` and `NameOwnerChanged`, and looks again every three seconds while on screen.
  `sway-things notifications dnd on|off|toggle` does the same from a key or a bar module. mako keeps its modes across
  `Reload`, so a change of the settings leaves Do Not Disturb on. The page asks for the owner of the name again before
  it switches, so a daemon that restarted meanwhile is the one switched.
- **Settings:** the registry has the page's settings (`notifications.*`); they have no sway directive except
  `notifications.blur` (SwayFX: `layer_effects` for `notifications`, which mako and dunst use, and swaync's
  `swaync-notification-window` and `swaync-control-center`, in `90-effects.conf`). Their apply mode is _tool reload_:
  `Applier::persist` writes our files from the stored values and, when one changed, has the daemon that runs read its
  config again (mako's `Reload`, swaync's `ReloadConfig` and `ReloadCss`). A value that isn't stored isn't written, so
  the daemon's own default applies; the defaults in the registry are the daemons' own (mako 1.11, swaync 0.12.6).
  Settings the daemons share (position, display, colors, clicking) have one key; the others are per daemon
  (`notifications.mako.*`, `notifications.swaync.*`).
- **mako** (`mako::generate`): our options are global (mako reads an included file as if it stood at the line, starting
  in the global section), then `[urgency=critical]` for its timeout and the Do Not Disturb section. The files are
  written when a setting of the page (or a window color it may follow) changed, and when the daemon chosen on the page
  lacks them, never on a change of another page: our file while mako is installed, has a config or is chosen (mako
  rejects its whole config over an include it can't find), and the user's mako config, with the include line alone, only
  for a mako chosen on the page and only where none exists, the running mako wasn't given one with `-c` and there is no
  system-wide `$XDG_CONFIG_DIRS/mako/config`; otherwise the page shows the line. The include line must be written
  `include=` without blanks and with a path that starts with `/` or `~/` (mako expands no variables); one written
  otherwise is reported, since mako refuses the whole config over it. When the folder of mako's config is a symbolic
  link (into a dotfiles repository), the page says where our file lands.
- **swaync** (`swaync::config`, `swaync::style`): `config.json` is swaync's own default (the installed
  `$XDG_CONFIG_DIRS/swaync/config.json`, else the one of swaync 0.12.6 bundled in the core), then the options an import
  passed through (`notifications.swaync.other`), then the stored settings; its first member `$comment` marks it as ours.
  With nothing stored it says what swaync's default says. `style.css` goes on top of swaync's packaged style sheet,
  which swaync always loads first; with nothing stored it is only a comment. The colors are written as named colors
  (`@define-color`), which both GTK 3 and GTK 4 read, and for swaync 0.12 (GTK 4) also as its CSS variables with the
  corners and the frosted preset (`frosted.css`). swaync 0.11 is built with GTK 3, which rejects the whole style sheet
  over a CSS variable, so it gets the named colors only; the version is the one the running swaync reports, else
  `swaync --version`. The panel's widgets and the buttons of its button grid are lists of names; the buttons run
  `sway-things hook quick <switch>`, so no shell code goes into the config. _Import My swaync Config_ and
  `sway-things notifications import-swaync` read the user's config (only read) into the settings and pass the options
  without a setting through.
- **Starting a daemon:** `notifications.start` puts `exec_always sway-things hook notification-daemon` into
  `80-hooks.conf`; the hook starts the chosen daemon with an argument list unless one runs, and notes its process id in
  `$XDG_RUNTIME_DIR/sway-things/notification-daemon.pid`. It writes our files first where they are missing (mako's, and
  swaync's config and style each); swaync gets them with `-c` and `-s`, or, if they can't be written, starts with its
  own config, which the hook reports. Choosing one in a session starts it at once over IPC. A daemon that runs and isn't
  the chosen one is ended first only if its process id is the one the hook noted; one the user started (a line of their
  config, a systemd unit, D-Bus activation) is left alone, and the page says how to switch. While a swaync config of the
  user's own exists and nothing of swaync's settings is stored, the page offers its import before swaync runs with ours,
  and `doctor` hints at it.
- **Clicking a notification shows its app** (`notifications.click_focus`, off by default): swaync runs
  `sway-things hook notification-click` as an action script (`run-on: action`, with `SWAYNC_DESKTOP_ENTRY` and
  `SWAYNC_APP_NAME` in its environment) beside the action itself; mako binds `on-button-left` to the hook with the
  notification's id, and the hook invokes the default action and dismisses it, as mako's own click does. The hook only
  talks to a sway whose socket (`SWAYSOCK`) is in the session's `XDG_RUNTIME_DIR`, next to the Wayland display the
  daemon shows on. It finds the window by comparing the desktop entry and the app name with each window's `app_id` and
  Xwayland class and instance, never its title (`click::find`: without case, also the last part of a reverse-DNS name
  and without `-desktop`; an exact name first, else one containing the other, four letters at least). Among several, the
  one on the focused workspace wins, then one on a visible workspace, then the others, the scratchpad last: sway keeps
  no history of focus. It focuses the window by its container id (`[con_id=…] focus`), reads the tree again and moves
  the pointer to the window's middle (`seat - cursor set`), not for one still in the scratchpad. The names are only
  compared, never put into a command. It closes swaync's panel and always exits 0, with the reason on stderr.
- **Not written for dunst yet:** dunst is asked and switched, but nothing is generated for it.

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
  reported (`Persisted::others`, kept by `generate::other`). For the default apps only the chosen apps are looked up,
  not every installed one, so a save stays cheap.

## Environment

`crates/core/src/environment/` writes Sway Things' environment.d file for the Environment page and every other page
whose settings are environment variables. The facts behind it are in the
[Sway Reference](Sway-Reference.md#environment-and-portals).

- **Variables from the registry:** a setting declares the names it owns and a function that makes its variables from its
  value (`SettingBuilder::environment`; a name ending in `*` owns every name it starts, for `LC_*`).
  `environment::variables` collects them in the order of the registry; a name written twice keeps the later value, and a
  variable with a name environment.d can't take or a value with a control character is left out. A value is written as
  it is where that is unambiguous, else in double quotes with `"`, `\` and `` ` `` escaped and `$` doubled
  (environment.d expands variables after taking the quotes away); a setting that checked a value in environment.d's own
  syntax can mark it to be written as it is.
- **The file:** `$XDG_CONFIG_HOME/environment.d/50-sway-things.conf`, with the header of the generated files, kept like
  the files of the Apps & Autostart page (created where none is, replaced while it is ours, deleted once no setting
  writes a variable; someone else's file is never touched). After it changed, `Applier::persist` runs
  `systemctl --user daemon-reload`, so the user manager reads environment.d again for the services it starts from then
  on and doesn't keep the old variables when it outlives a quick logout. sway and the apps it starts get them at the
  next login, so the settings apply after re-login.
- **Wayland for older apps:** four switches, off by default, each one variable whose value falls back to X11 where an
  app has no Wayland support: `QT_QPA_PLATFORM=wayland;xcb`, `SDL_VIDEODRIVER=wayland,x11`,
  `_JAVA_AWT_WM_NONREPARENTING=1`, `ELECTRON_OZONE_PLATFORM_HINT=auto`. Firefox, Chromium 140 and Electron 38 need
  nothing.
- **The user's own variables** (`environment.variables`, a list of `NAME=value`): written after the variables of every
  other setting, as typed. `environment::syntax` reads environment.d's syntax as systemd's generator does (comments,
  quotes only at the start of a value, backslashes, lines that go on after a `\`, invalid names and empty values
  ignored) and expands `$NAME`, `${NAME}`, `${NAME:-default}`, `${NAME:+alternate}` and `$$`. A typed value is held to
  more: one line, quotes closed, no `\` at its end, not empty, and every `${…}` closed and of those forms
  (`syntax::check_value`). The name is refused where the session sets it itself (`HOME`, `USER`, `LOGNAME`, `SHELL`,
  `XDG_RUNTIME_DIR`, `XDG_SESSION_*`, `XDG_CURRENT_DESKTOP`, `WAYLAND_DISPLAY`, `DISPLAY`, `SWAYSOCK`, `I3SOCK`,
  `DBUS_SESSION_BUS_ADDRESS`) and where a setting owns it (a switch of the page, `LANG` and `LC_*` of Region & Time);
  `PATH` must keep `$PATH` or `/usr/bin`. `PATH`, `LD_PRELOAD`, `LD_LIBRARY_PATH`, `XDG_CONFIG_HOME`, `XDG_DATA_DIRS`
  and `XDG_CONFIG_DIRS` are taken with a warning. The check is the setting's `check_text`, so the CLI and a hand-edited
  `settings.toml` are held to it too; the CLI splits a list at commas, so a value with a comma is set in the app.
- **Variables set elsewhere** (`environment::files`): every `.conf` file of `~/.config/environment.d`, `/etc`, `/run`,
  `/usr/local/lib` and `/usr/lib/environment.d` in systemd's order, with what Sway Things will write in place of its own
  file. For each line, the later line that replaces it (one that sets the name again without referring to it; one that
  refers to it, `PATH=$PATH:/x`, builds on it), and its value expanded in order from the user manager's environment
  (`systemctl --user show-environment`). The page shows them read only, with file:line, _Open in Editor_ for the user's
  own files, "Overrides yours" and "Overridden by …"; a row of a setting that writes variables (the switches, Region &
  Time's) says when a later file sets one of them again.
- **Does the session read it?** (`environment::session`) Only the user manager reads environment.d by itself; sway gets
  the variables only if whatever starts it runs the environment.d generator first. The page reads the environment of the
  sway that runs the session from `/proc/<pid>/environ` (the pid is in the name of `$SWAYSOCK`, the start is the
  socket's time) and, when a variable of the file is missing there although the file is older than sway, shows a notice
  with the line to run before sway starts (`set -a; eval "$(<generator>)"; set +a`, with the generator of the system) to
  copy. Variables in environment.d's own syntax aren't compared, since only the generator knows what they expand to.

## XDG desktop portals

`crates/core/src/environment/portals/` checks what screen sharing, screenshots of other apps, the file pickers of
sandboxed apps and the dark style need on sway, for the Environment page and `sway-things doctor`. The facts are in the
[Sway Reference](Sway-Reference.md#environment-and-portals).

- **Installed:** xdg-desktop-portal by its D-Bus service file (`$XDG_DATA_HOME`, `$XDG_DATA_DIRS`) whose `Exec` program
  exists, and xdg-desktop-portal-wlr and -gtk by their `.portal` file and their service. A missing one is shown with its
  package and the install command of the distribution (`packages::Manager` from `os-release`'s `ID` and `ID_LIKE`:
  zypper, dnf, apt, pacman) to copy; nothing is installed.
- **Configured:** the backends from the `.portal` files of `$XDG_DATA_HOME`, `$XDG_DATA_DIRS` and `/usr/share`, sorted
  as xdg-desktop-portal sorts them; the configuration that wins, searched as it searches; and the backend of
  `ScreenCast`, `Screenshot`, `FileChooser` and `Settings` under it, with the `UseIn` and gtk fallbacks. It needs a fix
  when `ScreenCast` isn't wlr or nothing serves `FileChooser`. `XDG_DESKTOP_PORTAL_DIR` narrows the search to one
  directory, as for xdg-desktop-portal.
- **The fix in our own file:** `environment.portal_config` writes `~/.config/xdg-desktop-portal/sway-portals.conf` with
  the header of the generated files and the choices Arch and Fedora ship with sway (`default=gtk`, wlr for `ScreenCast`
  and `Screenshot`, `Inhibit=none`), kept like the other files of other programs. It is only written where the user has
  neither a `sway-portals.conf`, nor the `<desktop>-portals.conf` of a desktop of `XDG_CURRENT_DESKTOP`, nor a
  `portals.conf` there: in that directory ours would win over them. With such a file, the page shows it read only with
  _Open in Editor_ and the lines to copy. After the file changed, `Applier::persist` runs `systemctl --user try-restart`
  of xdg-desktop-portal, -wlr and -gtk, so the running portals read it; those that don't run read it when they start.
- **Session variables:** `systemctl --user show-environment` must have `WAYLAND_DISPLAY` and `XDG_CURRENT_DESKTOP`,
  since the user manager starts the portals. `environment.pass_session_variables` generates
  `exec systemctl --user import-environment …` and `exec dbus-update-activation-environment --systemd …` into
  `70-autostart.conf`, with the fixed names `sway-things hook autostart` hands over too
  ([Apps and autostart](#apps-and-autostart)), written as keywords without quotes; applied live, sway runs them at once.
  A line of the user's config that runs `systemctl --user import-environment` reads as the switch on.
- **Running:** `org.freedesktop.portal.Desktop` has an owner on the session bus (`NameHasOwner`, which starts nothing).
  The portal is D-Bus activated, so not running is a warning: it starts when an app asks.
- **Output chooser** (`environment::screencast`): how xdg-desktop-portal-wlr asks which display to share,
  `environment.screencast_chooser`: _Default_ writes nothing, so xdpw picks the first chooser it finds; _slurp_ writes
  `chooser_type=simple` and `chooser_cmd=slurp -f %o -or`; _menu_ `chooser_type=dmenu` with the fixed command of the
  first launcher installed (`fuzzel --dmenu`, `wofi --dmenu`, `rofi -dmenu`, `bemenu`, `wmenu`); _output_
  `chooser_type=none` and `output_name` from `environment.screencast_output`, which takes only letters, digits, `.`, `_`
  and `-`. xdpw runs `chooser_cmd` through `sh -c`, so only these fixed commands are written. Where the tool isn't
  installed or no display is chosen, no file is written and xdpw picks itself; the page shows such an option insensitive
  with its reason and the install command to copy. The file is our own `~/.config/xdg-desktop-portal-wlr/sway`, kept
  like the other files of other programs and only written where the user has neither a `sway`, nor the file of another
  desktop of `XDG_CURRENT_DESKTOP`, nor a `config` there; with one, the row is read only and names it with _Open in
  Editor_. The page says when ours replaces a file in `/etc/xdg/xdg-desktop-portal-wlr`, and when `XDG_CURRENT_DESKTOP`
  doesn't name sway, so xdpw wouldn't read it. After the file changed, `Applier::persist` runs
  `systemctl --user try-restart xdg-desktop-portal-wlr`.
