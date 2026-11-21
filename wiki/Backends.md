# Backends

> Part of the [architecture](Architecture.md). The services and tools Sway Things drives besides sway: BlueZ,
> NetworkManager, the sound server and GSettings, and the planned backends for the companion tools.

The plan (not built yet) is a backend trait for each area that has competing tools. Today the only companion tool Sway
Things drives is kanshi, in `crates/core/src/outputs/kanshi.rs` ([Outputs](Outputs.md)); Bluetooth, the network and
sound are services with no competing stack and use no trait (below).

```rust
// planned
trait Backend {
    fn detect(&self) -> Availability;   // installed? running? version? managed externally by file:line?
    fn read(&self) -> Result<Values>;
    fn apply(&self, change: &Change) -> Result<()>;
    fn generate(&self, state: &State) -> Vec<GeneratedFile>;
}
```

| Area | Backends (reference first); only display profiles, audio, Bluetooth and the network are built |
| --- | --- |
| Display profiles | kanshi, per combination of displays; without kanshi: static `output` lines in `20-output.conf` (no profiles); shikane later |
| Bar | waybar, swaybar (`bar {}` block) |
| Idle / lock | swayidle + swaylock |
| Notifications | swaync, mako, dunst (Do Not Disturb first) |
| Night light | gammastep, wlsunset |
| Wallpaper | swaybg via `output * bg`; swww later |
| Launcher | fuzzel, wofi, rofi(-wayland), tofi |
| Screenshots | grim + slurp (grimshot), swappy / satty |
| Clipboard history | cliphist, clipman |
| Auto dark mode | darkman |
| Audio | PulseAudio API via pipewire-pulse or PulseAudio (`libpulse-binding`, on libpulse's threaded main loop) |
| Autostart | our `exec` lines + hand-written `exec` (read-only) + XDG autostart with runner detection |
| Bluetooth | BlueZ (D-Bus) |
| Network | NetworkManager (D-Bus) |
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
