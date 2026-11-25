# UI Architecture

> Part of the [architecture](Architecture.md). How the GTK app is built: the window, pages built from the registry, the
> custom pages, and translations. The behavior it implements is in the [UX Concept](UX-Concept.md).

- Plain **gtk4-rs**. No libadwaita, because it overrides the user's GTK theme. No Relm4, because standard rows are
  generated from the registry anyway, and custom widgets need plain gtk-rs.
- `GtkApplication` with app_id **`io.github.mowi12.SwayThings`**, unique on the session bus: starting
  `sway-things-gui [<link>]` again hands the command line to the running instance (`HANDLES_COMMAND_LINE`), which opens
  the [deep link](Architecture.md#settings-registry) and presents its window. An unknown link goes to the caller's
  stderr with exit status 2 and changes nothing. The action `app.open` (a string, empty to only present the window)
  opens a link too, also through `gapplication action io.github.mowi12.SwayThings open "'keyboard'"`. A link that comes
  while the window loads is opened once it is there. `Window::open_link` opens the page and, for a setting, points to
  its row: its group expanded, scrolled into view, its control focused, and tinted with the accent for 1.5 s before it
  fades (at once without animations). A row that isn't shown, like one of an input type without devices, leaves just the
  page open. Without a session bus every start runs on its own.
- **Rows from the registry:** a `SettingRow` per setting, with title, subtitle, the config line in monospace below the
  subtitle while `app.show_directives` is on (off by default; every row follows a switch at once), where a value from
  the user's config comes from, an error under the title when a change failed, and the control for its value type: a
  switch (on/off), a slider over the setting's slider range plus a number field for every value it accepts (whole and
  decimal numbers; the slider marks the default and snaps to it within 2 % of its range while dragged with the pointer,
  keys and the field stay exact), a drop-down (choice), a color button (color) and an entry, applied on Enter or when it
  loses the focus (text). A text setting with known values (`SettingRow::with_choices`, the keyboard models) gets a
  drop-down with search instead, which shows a value typed by hand at its end. A list gets a row without a control, and
  its page shows its own widget below the row, which follows the row's device (`connect_device_changed`) and changes the
  value through it (`SettingRow::change`), so refusals show at the row. A locked setting is insensitive, with the reason
  as tooltip and accessible description.
- **Pages from the registry:** `PageView::for_settings(model, page)` builds a page from the registry: a heading and a
  bordered list per section, and the generated files the page writes to at the bottom. Every page with settings is built
  this way; custom pages can mix these rows with their own widgets.
- **Page parts** (`crates/gui/src/widgets/page_view/parts.rs`): what a page adds to its rows, in one place, so
  `PageView::for_settings` names no page: the parts above the rows (About's system details and generated files, the list
  of bindings on Keyboard Shortcuts, Apps Look & Feel's note, the Displays arrangement, Do Not Disturb on
  Notifications), below them (the test area, the workspace map, the note above the default apps and the list of what
  starts at login on Apps & Autostart; on Notifications, the sections of mako and swaync follow the daemon the page is
  about), the choices of a row and what it follows (a Keyboard Shortcuts or Apps & Autostart setting may have no row),
  and the widgets below a row (the Keyboard page's layouts and options, what a media key switch replaces, what keeps the
  terminal's and launcher's keys from the choice, the notes of Screenshots & Clipboard, the lists of swaync's widgets
  and buttons), and the generated files a page writes to besides those of its directives. About's sections are
  `page_view/about.rs`, the per-device sections `page_view/device_group.rs`.
- **Grouped rows:** the members of a `SettingGroup` go below a `GroupRow` in their section's list: the group's title, a
  summary of the members' values (a swatch per color, else the values joined), the shared config line once while
  `app.show_directives` is on, and a chevron. Its list item takes the focus; a click, `Enter` or `Space` expands it, and
  its accessible state says whether it is expanded. The members' `SettingRow`s are indented list items, hidden while the
  group is collapsed, and never show the config line (`SettingRow::set_grouped`). `PageView::row` counts a member of a
  collapsed group as shown, and `PageView::point_to` expands the group before it points to the member.
- **Indicators** (`SettingRow`): for a stored value of its own, a reset button (`Model::reset`; a risky setting through
  `Applier::try_risky_reset`) and a toggle for the host overlay (`Model::move_to`). A conflict comes from
  `conflict::Report` (`Model::report`) while the value is stored: the line that wins over it (`overriding`), else the
  last one it made dead (`overridden`), shown as a badge and a ⋮ `MenuButton` whose popover has the line and _Let my
  config win_ (`Applier::let_config_win`), _Open config in editor_ (see [Editor](Reading-Sway.md#editor)) and _Copy
  line_. What the compositor lacks is a badge too, and so is "Applies after reload" for a setting sway only reads from
  its config. The page header's _Reset page…_ (`Model::page_reset`, `Model::reset_page`) asks with a `gtk::AlertDialog`
  and resets every stored value of the page except those of risky settings. A page with a body of its own (Wallpaper)
  adds it with `PageView::add_page_reset`.
- **Per-device sections:** each run of sections of one input type starts with a `DeviceSelector` row (all devices of the
  type, or one of `Model::devices`), which points the rows of those sections at a device with `SettingRow::set_device`.
  A row for one device without its own value notes that it is the type's value. While the devices are known and none of
  the type is listed (connected or with stored values) and nothing is stored for the type, the run shows its first
  heading and a hint instead of the lists. The Mouse & Touchpad page ends with a `TestArea` (a pad that names the last
  click, a strip to scroll).
- **Per-output sections:** a run of sections of per-output settings that don't set a display up starts with the same
  selector for the outputs (`DeviceSelector::of(Instances::Outputs)`: all displays, or one of `Model::outputs` by its
  identifier, shown by name and model), and a row for one output notes that it uses the value of all displays. The
  Displays page picks its output on the arrangement instead. The value for all displays applies to screens plugged in
  later, so these sections never give way to a hint.
- **Wallpaper page** (`crates/gui/src/widgets/wallpaper/`: `preview`, `stage`, `pictures`, `collections`, `colors`,
  `images` and `notices` around the state in `mod.rs`), a page of its own rather than rows: the notices about wallpaper
  tools; a preview with a tile per output that shows a wallpaper (on, not mirroring another), with its wallpaper and the
  output's number as on the Displays page in a corner, in a `Stage` that places the tiles in `size_allocate` with the
  canvas's `View` (`displays/canvas/view.rs`: the room around the layout, the fit and the pixel between tiles, shared so
  both pages place the outputs alike); _Same wallpaper on all displays_, which edits the values for every output, or
  while off those of the tile clicked (switching back removes the values of single outputs, after asking); what the
  edited output shows, with an icon and a line for its kind (picture, collection or color), _Next Picture_ for a
  collection, the fit as linked toggle buttons and the color. The default of a display is what it shows once Sway Things
  stores no wallpaper for it (`Fallback`): for one display, the wallpaper of all of them if that is stored; else the
  `bg` line of the user's config (`Model::report`); else sway's, none. While it shows, a dim line says where it comes
  from; else _Use Default_ (its tooltip names the default) removes the display's stored values, with
  `Model::let_config_win` so the user's line is sent at once, or for a display that falls back to all of them with
  `Model::reset`. A change of the fit or the color while the user's picture shows first stores the picture, fit and
  color of their line (`take_over`), so the generated line draws their picture with the change. A picture whose file is
  missing or can't be decoded (`images::when_readable`; swaybg reads pictures with the same gdk-pixbuf loaders) gets a
  warning below, naming `webp-pixbuf-loader`; such a tile shows an icon in place of the thumbnail. The tiles draw the
  fit as swaybg does: `center` and `tile` show the picture at its own size against the output's pixels (a
  `gtk::DrawingArea` with cairo). Below, tabs: the pictures of `app.wallpaper_folder` (else the XDG pictures folder,
  `~/Pictures`) and of its subfolders, which become filter chips, after the _Default_ tile (the default's picture or
  color, selected while it shows; a click does what _Use Default_ does), with _Choose Image…_; the collections as cards,
  with the settings and pictures of the open one (_Add Pictures…_, _Add Folder…_, a click removes one, _Delete
  Collection…_); and colors. The tab's buttons sit at the end of the row of the tabs (a `RowLayout`). Every tile of the
  grids is a 16:10 frame that the picture fills, cut off where it doesn't fit; a folder or an empty collection shows the
  background color of the edited output (`images::frame`, one style sheet swapped when the color changes). Folders are
  listed and thumbnails decoded and scaled off the main loop (`gio::spawn_blocking`; a pixbuf can't leave its thread, so
  its pixels become a `gdk::MemoryTexture`), once per picture. A picked picture is stored with `~/` for the home folder;
  picking one, or a color, stops a collection on that output.
- **Keyboard page** (`crates/gui/src/widgets/keyboard.rs`): `PageView` adds the page's own widgets below some rows. The
  XKB rules come from `crates/gui/src/xkb.rs`, read once. Below the layouts, a `LayoutList` (a row per layout with a
  drag handle, its name, a drop-down of its variants and ✕; Alt+↑/↓ move the focused layout and Delete removes it; the
  last layout can't be removed) and _Add layout…_, a popover that searches the layouts and variants by name,
  description, language and country (exotic ones only in search results). Every change tries the layouts and the
  variants as one change (`Model::try_risky_all`). _Active layout_ lists the layouts sway reports for the keyboard (for
  all keyboards, the first usable one) and switches them (`Model::switch_layout`, only in a session). The keyboard model
  is a drop-down of the rules' models, and _Try it_ is a field to type in at the end of the section.
- **Keyboard Shortcuts page** (`crates/gui/src/widgets/shortcuts/`): `PageView` starts the page with a `ShortcutList`
  section: a search field, _Add Shortcut…_, and a `ListBox` per group (the categories, "other" under "system", then one
  per mode) of the rows of `Model::bindings` (the core's `Listing` for the model's status and state, with a `GdkKeymap`
  from `gdk::Display::map_keycode`), rebuilt when the model changes. The rebuild keeps the place on the page
  (`widgets::Place`, also used by the `WorkspaceMap`): before the rows go, it notes the scroll position and where the
  focused widget is in the page's content, and takes the focus off (a focused widget that is removed makes GTK move the
  focus to the next one of the window, which scrolls the page there); after, it gives the focus to the row of the same
  binding (or the next one) and, in a handler of the frame clock's `layout` signal that runs after the window's, scrolls
  so that row is where the old one was, or to the old position after a removal. Several rebuilds before the next frame
  (the model says it changed when a change is queued and when it is applied) share the place taken first. A row
  (`row.rs`) is an activatable `ListBoxRow` with one line: what the binding does (`Row::description`, else the command)
  and the keycaps, between them only a warning sign (not in effect), a lock (`locked_reason`) or the ↶ button
  (`bindings::edit::restore`), the reasons as tooltips and accessible descriptions. Activating a row opens the `Editor`
  dialog for it, a modal `gtk::Window` with the keys, the line that says whether they are free, the clashes, the action,
  the details (`gtk::Expander`) and _Remove_ / _Back to Your Config's Binding_ / _Save_; their changes store the list
  `bindings::edit` makes (`Model::set` of `shortcuts.bindings`). A binding that isn't `Row::editable` opens it read
  only, with a button to the setting that owns it (`owner`, opened through `Window::open_link`). Its key field listens
  with an `EventControllerKey` in the capture phase and asks for every key (`gdk::Toplevel::inhibit_system_shortcuts`)
  while it does, taking the keysym of the key's first level in the event's layout, so Shift+1 is `Shift+1` as sway's raw
  keysyms see it; the modifiers held meanwhile list the keys taken with them. The apps it offers are
  `gio::AppInfo::all()` with the field codes of their command lines left out. The list item has no row; a media key
  switch has one only while its program is installed or it is stored, with what its keys clash with below it; the switch
  of its on-screen display only below a shown switch, while `swayosd-client` is in `Model::search_path` (`$PATH`, or a
  directory a test sets) or it is stored, with the note of `shortcuts/osd.rs` below it (`swayosd::server`, read again on
  each change of the model, when the note is mapped and when the window becomes active).
- **Screenshots & Clipboard page** (`crates/gui/src/widgets/capture.rs`): rows from the registry, with the page's parts
  through `page_view/parts.rs`. A row of keys gets a `KeyButton` in place of its text field (bound to the field's
  sensitivity), which listens like the Keyboard Shortcuts dialog's field (`EventControllerKey` in the capture phase,
  `inhibit_system_shortcuts`, the shared `shortcuts::capture`) and stores the keys after `check_keys`; below it, the
  clashes of its `RowKind::Feature` row in `Model::bindings`. Below the switches: the missing programs and their
  packages (`capture::tools`), the launcher, recorder and editor in use; below a file name, the next name (every second
  while on screen); below the history, the password note, the user's own history (`clipboard::own_history`, or a foreign
  watcher) and _Clear History…_ after an `AlertDialog`; below the recording switch, while `record::running` finds a
  recording (looked for every two seconds while on screen), _Stop_, which runs `sway-things record`. The page has the
  history's watcher follow its switch and size (`clipboard::sync_with`) once they stood still for 0.8 s. Its footer
  names `50-bindings.conf` and `80-hooks.conf`.
- **Brightness & Night Light page** (`crates/gui/src/widgets/brightness.rs`, `night_light.rs`): `PageView` starts the
  page with a "Brightness" section (`brightness::panel`), then the night light's rows from the registry with its parts
  through `page_view/parts.rs`. The panel builds a row for the screen's backlight and one for the keyboard light from
  `brightness::devices`, sends through `Logind` one request at a time (the newest value waits) and reads sysfs every
  second while on screen, a slider moved in the last 1.5 s keeping its place. Each time it is mapped, `ddc::detect` and
  `ddc::get` run in `gio::spawn_blocking`; a monitor's slider sends `ddc::set` 0.5 s after it stood still. The notes
  below the night light's switch rebuild only when what they say changes, and walk the switch's list to show the rows of
  the schedule chosen and lock them while a night light of the user's runs or no tool is installed (every three seconds
  while on screen, and at each change of the model); the page has the tool follow the settings
  (`night_light::sync_with`, in `gio::spawn_blocking`) once they stood still for 0.8 s. Its footer names
  `80-hooks.conf`.
- **Displays page** (`crates/gui/src/widgets/displays/`): `PageView` starts the page with an `Arrangement` section, a
  `Canvas` of toggle buttons (a tile per output, placed in `size_allocate` by the layout scaled to fit with `View`, the
  dragged one drawn last, and two guide lines as widgets in the accent) with a `GestureDrag` in the capture phase, so a
  press that moves becomes a drag instead of a click. Snapping, settling, the slot between two outputs (`slot_at`,
  `insert`) and moving the layout to 0,0 come from `outputs::layout`; after a drop the tiles slide to their places in a
  tick callback (200 ms, eased; not with `gtk-enable-animations` off), and so do the others when a dragged tile comes
  over a slot or leaves it (the drag keeps where `place` would put them, worked out only when the slot changes). The
  picked tile has a grip in its corner: dragging it projects the movement onto the tile's diagonal, so the tile keeps
  its proportions, and stores the scale on release; a drop stores the position of every output that is on
  (`Model::set`), shows the dropped layout until sway reports it or for three seconds, and keeps the arrangement before
  it for Ctrl+Z (on the whole page). While a risky change waits, the tiles can be picked but not moved
  (`Canvas::set_locked`), and a trial that changes a display's size has the others follow it at once: the canvas works
  out the flowed layout against the one before the trial and sends their new places with `Applier::move_along`, which
  records how to put them back in the trial's undo and stores them on Keep (`Pending::along`); `Applier::try_risky_all`
  gives a display that is switched on a place right of the others in the same trial. Revert puts back everything that
  changed. When outputs' sizes change with what is stored for them, the layout flows so that what touched still touches
  (`outputs::layout::reflow`, for one output or several, like a scale for all of them); the layout to compare with is
  held while the model is busy or a risky change waits, and for up to three seconds until what is stored catches up with
  sway's report, so a change from elsewhere (`swaymsg`) moves nothing. An `OutputPicker` before the rows of the outputs
  picks the output they are for (`SettingRow::set_device`), and the mode row has two drop-downs whose entries follow the
  output: the resolutions (`SettingRow::set_choices`, each a whole mode) and beside it the refresh rates of that
  resolution (`SettingRow::add_control`), each a whole mode as sway reports it, so no rate is rounded. The model keeps
  an `OutputWatch` like the `DeviceWatch`, and gives the rows the outputs as `Known`, so a value nothing sets shows what
  sway chose. _Identify_ shows each output's number on it for three seconds: a window per output on the overlay layer of
  `wlr-layer-shell` (gtk4-layer-shell), on the GDK monitor whose connector is the output's name, taking no keyboard and
  no input region; the tiles are outlined in the accent meanwhile. Without `wlr-layer-shell` (not sway) the button can't
  be pressed and says why.
- **Apps & Autostart page** (`crates/gui/src/widgets/apps/`): the default app, terminal and launcher rows are drop-downs
  of the installed apps, drawn with their icons by factories of their own (the open list adds a line below an item, such
  as where the default comes from). Their choices follow the model (`SettingRow::set_choices`, from `apps::below`): the
  first item, no choice, is named after the app or command in effect without one, which isn't offered twice. The note
  below a row says only what goes wrong. The keys of the terminal and launcher use the button of keys of Screenshots &
  Clipboard (`capture::follow`). The `AutostartList` (`apps/autostart.rs`), above the runner's row in the "Autostart"
  section, puts Sway Things' entries, the XDG autostart entries and the user's `exec` lines in one section list
  (`widgets::list`) and the entries that don't start in this session in a second one in an expander, a line each,
  rebuilt when the model changes; its switches change `autostart.entries` and `autostart.hidden` item by item
  (`Model::list`, `Model::set`). A click on a line opens its details (`apps/dialogs.rs`), where _Remove_ changes
  `autostart.entries` and _Start Now_ sends the entry's `exec` (`Model::run_command`); _Add App…_ and the command it
  asks for use the Network page's `Dialog`. The two lists have no rows of their own.
- **XKB options** (`crates/gui/src/widgets/xkb_options.rs`): below the options row, drop-downs for what Caps Lock does
  (`ctrl:nocaps`, `caps:escape`, `caps:swapescape`, `caps:none`), the Compose key (`compose:`) and the layout switching
  shortcut (`grp:`), a switch for `ctrl:swap_lalt_lctl`, and the `OptionBrowser`: every option of the rules by group,
  with a search (exotic ones only in search results), check buttons, and radio buttons with _None_ for groups whose
  options exclude each other. Each changes the one options list through the row, so each change goes through the safety
  net, and options they don't know stay as they are.
- **Live pages** (`crates/gui/src/widgets/bluetooth/`, `network/`, `sound/`): Bluetooth, Network and Sound show state
  that a service owns (BlueZ, NetworkManager, the sound server), not settings, so they aren't built from the registry.
  `Window::build_page` builds each one around a handle from the core ([Backends](Backends.md)), hands it the one
  preference it follows (`app.bluetooth_search`, `app.wifi_search`, `app.sound_above_100`) and puts its _Advanced…_
  button, which starts the specialist tool, in the page header. Each page follows the service's change stream on the
  main loop (`glib::spawn_future_local`, holding the page by a `WeakRef`), reads a new snapshot at most every 200 ms
  (Sound 100 ms, `live::follow_changes`) and updates its rows in place; the tasks end in `dispose`. The sections, rows
  and hints are the shared ones of `widgets/list.rs` (the notices of every page are `widgets/notice.rs`), the forget
  dialog is `live::confirm_forget`, and `window::live_view` puts each page under its title with its _Advanced…_ button.
  Bluetooth and Network search only while the page is on screen and the radio is on, or after _Search_ with the
  preference set to manual. Their dialogs (`dialogs.rs`) take the pairing agent's and the secret agent's questions
  (Wi-Fi passwords, a VPN plugin's fields, the steps of an OpenConnect sign-in), renaming, passwords, hidden networks
  and forgetting. Device, network and app names are untrusted and only ever set as plain label text. The tests run the
  pages against the stand-ins of `bluetooth::stand_in`, `network::stand_in` and `sound::stand_in`.
- **Printers page** (`crates/gui/src/widgets/printers/`; its rows in `rows.rs`, what they do in `actions.rs`, the
  dialogs for adding a printer, its details and removing it in `dialogs.rs`, the ink and toner bars in `supplies.rs`):
  live like the pages above, but CUPS has no change stream to follow, so it reads a snapshot every 2 s while it is on
  screen and its window isn't suspended, at once when it comes on screen, and after each action
  ([Backends](Backends.md)). `window::live_view` puts it under its title with its _Advanced…_ button
  (system-config-printer, else CUPS's web interface), and its sections, rows and hints are those of `widgets/list.rs`.
  Printer names, models, locations, job titles and owners are only ever set as plain label text. The tests run it
  against the stand-ins of `printers::stand_in`.
- **Notifications page** (`crates/gui/src/widgets/notifications.rs`): the Do Not Disturb section's panel (the switch,
  the daemon that runs and what started it, _Send Test Notification_ and the notes with what to do) and the checklists
  below the rows of swaync's widgets and buttons. The panel reads the daemon through `Model::daemon_bus` on each change
  it reports (`live::follow_changes`), every three seconds while on screen (mapped, in a window that isn't suspended on
  a hidden workspace) and after every change of the settings, one read at a time (asked during one, it reads once more
  after it), and tells the page which daemon it is about, so only that daemon's sections show
  ([Notifications](Backends.md#notifications)).
- **Workspaces page** (`crates/gui/src/widgets/workspaces.rs`): below the rows of `workspaces.count` and
  `workspaces.placement`, a map with a card per display that takes workspaces, holding the numbers the placement rule
  gives it (`Model::placement`). A click on a number sends it to the next display, a drag onto another card puts it
  there; either makes the placement fixed for this combination (`Model::fix_placement`), see
  [Outputs](Outputs.md#workspace-placement). While sway places the workspaces itself, the map is hidden.
- **Cursor & Tablet and Apps Look & Feel** (`crates/gui/src/widgets/cursor.rs`, `look.rs`): rows from the registry with
  page parts that `PageView` adds: drop-downs of the installed cursor themes (each with its arrow) and of the sizes the
  picked theme ships, of the screens a tablet or touchscreen can map to (following the outputs), of the installed GTK
  themes and of the icon themes (each with a few of its icons); and notes at the top of Apps Look & Feel when GSettings
  has no `org.gnome.desktop.interface` or no Settings portal reports the style ([GSettings](Backends.md#gsettings)).
- **Model** (`crates/gui/src/model.rs`; its queue of operations in `model/queue.rs`, the watchers in `model/watch.rs`,
  what it sees of kanshi in `model/kanshi.rs`): owns the core's `Applier`, the include status and the watchers of the
  user's config and the settings files. In a session it also runs a `DeviceWatch` on a connection of its own, and lists
  the devices with stored values after every change (`Devices::set_stored`); without one only those are listed, and
  `devices_known` is false. Rows read from a copy of the state, so they never wait for a change in progress. A change is
  queued and sent one at a time; a setting that changes again before its turn sends only its newest value (a slider
  being dragged). The files are saved `SAVE_DELAY` after the last change. The model reports `changed` (rows read again;
  a row with a newer value on its way keeps it), `failed` (per setting) and `problem` (shown in a banner above the page:
  a generated file sway rejected or that was changed outside).
- **Welcome screen and status banner:** while the include status isn't active, the window shows `Welcome` in the content
  pane instead of the page: the core's explanation of the status (`IncludeStatus::explanation`, without the line), the
  line with a copy button, a button that opens the config in the user's editor (`editor::open`, see
  [Editor](Reading-Sway.md#editor); the default application only when no editor is found) and a spinner while it waits.
  _Look around first_ or opening a page leaves it for the pages, with a banner above them that leads back. The window
  follows the model's `changed` signal, so the welcome screen and the banner go away as soon as the watcher finds the
  line. Once active, a banner tells how many of the user's lines come after the include, until dismissed.
- Custom pages are GObject subclasses.
- **Components:** every building block is a widget in `crates/gui/src/widgets/`, a GObject subclass written in Rust (no
  `.ui` files) with its own CSS node name (`st-sidebar`, `st-page-header`, …). The shell is made of them: `Sidebar` (the
  pages of `Page::ALL` under their group headings), `Panes` (the sidebar and the content pane side by side, or below
  `NARROW_WIDTH` in `window.rs` or the width both need, one at a time; the other pane is unmapped), and per page a
  `PageView` (header, body, scrolled, at most 760 px wide through `Clamp`); a page without settings that isn't built yet
  shows a `Placeholder`.
- Our own CSS (`crates/gui/src/style.css`) covers structure only. Colors come from the GTK theme. Secondary text uses
  `.st-dim` (opacity 0.75) instead of the theme's `.dim-label` (0.55), which stays below 4.5:1 in GTK's own theme.
- States that carry meaning (a display that is off on the canvas) are in `crates/gui/src/style-states.css`, loaded above
  the user's own `gtk.css` (`STYLE_PROVIDER_PRIORITY_USER + 1`), which otherwise styles every button alike.
- Icons the themes don't have (the globe, code brackets and bug of the project links) are Lucide's, as symbolic SVGs in
  `crates/gui/resources/` that GTK recolors with the text color. `build.rs` compiles them into a GResource bundle in the
  binary, which `icons::register` adds to the icon theme at startup, so nothing extra is installed.
- **Theming** (`crates/gui/src/theme.rs`): the accent is the `windows.focused.child_border` value a row shows (stored,
  else the user's config line, else sway's `#285577`). GTK's own theme compiles its accent into literal colors, so a
  generated stylesheet above the app's own colors the accented parts (checked switches and check buttons, slider fills,
  selections, suggested actions, focus rings, links) and sets the theme's named accent colors. Fills get the exact color
  with black or white text, whichever contrasts more; where the accent is drawn on the window background it is shifted
  in lightness until it reaches 4.5:1 there. The stylesheet is rebuilt when the color, `app.accent_source` or the
  light/dark style changes; with `app.accent_source = "theme"` it is empty. GTK 4.20 and newer follow the desktop's
  light/dark preference; before 4.20 the app reads `org.freedesktop.appearance color-scheme` from the XDG Settings
  portal over GIO's D-Bus and follows its changes. The WCAG math (luminance, contrast ratio, text on a fill, a color
  made readable on a background) lives in `crates/core/src/theme.rs`, shared with the GUI's contrast check.
- **Concurrency**: one event loop. Futures run on the GTK main context (`glib::spawn_future_local`); I/O uses
  executor-agnostic, async-io based crates (zbus, async-process, and our own IPC wire format on async-io); the CLI uses
  `async_io::block_on`. No tokio. `crates/core` must not depend on a specific executor. This way the UI updates directly
  in futures without bridging threads, the dependency tree stays small, and a CLI started as a hook comes up fast.

## Internationalization

gettext from day one. Source strings (msgids) are American English; en_GB and other languages are translations.

- **Marking**: a translatable string is a `Msg` (`crates/core/src/i18n/`), written as `msg!("…")`, which works in
  constants too. The registry's builder methods (`title`, `subtitle`, `section`, `group`, `keywords`, `placeholder`) and
  `Choice::new` take `Msg`, so an unmarked literal doesn't compile. Registry titles, subtitles, sections, group titles,
  keywords, placeholders and choice labels, page and sidebar group titles and descriptions, the headings of the doctor's
  report and its findings, the core's errors, the explanation of the include line, the labels and values of the system
  details, the reasons a row is locked (with the titles and choice labels in them) and the messages of the command line
  (`error:` and `warning:` too) are marked; the GUI's texts are marked too. The command line and the GUI show the core's
  messages and errors with `translated()`. Texts from other programs (sway's errors, the messages of BlueZ and
  NetworkManager, the names the sound server gives its devices, the system's I/O errors) stay as they are. The block of
  system details that About copies for a bug report stays English, for the maintainers who read it. The command line's
  `--help` texts stay English: the man pages and completions are generated from them, and clap's own words (`Usage:`,
  `Options:`, its errors) can't be translated, so a translated help would mix two languages. `--json` output and its ids
  stay English too. A `// TRANSLATORS:` comment above a string becomes a note in the template. Ids derived from a
  string, like the section ids in `ui.toml`, use the English msgid, never the translation.
- **Template**: `scripts/update-pot.sh` extracts the marked strings with `xgettext` (0.24 or newer, the first version
  that reads Rust) into `po/sway-things.pot`, with file names but no line numbers, so the template only changes when the
  strings do. `scripts/check-pot.sh` fails while the template is out of date, and a core test
  (`crates/core/tests/pot.rs`) fails when a string of the registry or the doctor is missing from it. CI runs the check
  in an Arch Linux container, because Debian 13 and Ubuntu 24.04 ship older gettext versions.
- **Desktop entry and metainfo**: their translatable fields (name, generic name, comment, keywords, summary and
  description) go into the same template: `scripts/update-pot.sh` extracts them from `data/*.desktop.in` and
  `data/*.metainfo.xml.in` in a second `xgettext` run. At build time, `msgfmt --desktop` and `msgfmt --xml` merge the
  translations of the languages in `po/LINGUAS` into the installed files.
- **Lookup**: each binary calls `i18n::init()` at startup. It reads the catalogs (`.mo` files) of the text domain
  `sway-things` with a small reader of its own, in pure Rust: no C library, no `setlocale`, no global state outside the
  one catalog set. The catalogs are `<dir>/<language>/LC_MESSAGES/sway-things.mo`, where `<dir>` is
  `$SWAY_THINGS_LOCALEDIR`, else the directory the build set in the same variable, else `/usr/share/locale`. The
  languages are picked as gettext picks them: the list in `LANGUAGE`, else `LC_ALL`, `LC_MESSAGES` or `LANG`; nothing is
  translated in the C locale (also `C.UTF-8`). `de_AT.UTF-8` is tried as `de_AT.UTF-8`, `de_AT`, `de.UTF-8` and `de`,
  and a string missing from the first language's catalog is looked up in the next.
- **Names of the XKB rules**: the models, layouts, variants and options the Keyboard page shows come from
  xkeyboard-config, which translates them in catalogs of its own (text domain `xkeyboard-config`, in
  `/usr/share/locale`; xkb-data on Debian and Ubuntu, xkeyboard-config-lang on openSUSE). `init` loads them for the same
  languages, and `Item::shown` gives a name in the user's language, `Item::label` the English one. Search matches both;
  without the package's translations the names stay English.
- **Messages of the core**: a message the core makes for people and scripts alike (a finding of the doctor, why a row is
  locked, an error) is an `i18n::Text`: the marked `Msg` (or `MsgPlural` and its count) with its values, each data shown
  as it is (a file name, a number) or a `Text` or `Msg` of its own, translated with it (a setting's title, the error
  that caused this one). `text.english()` gives the English text for logs and `--json`, `text.translated()` the text in
  the user's language for display, and `Text::data` holds text that is never translated (a line of the user's config,
  what another program said). An error has `fn text(&self) -> Text`, and its `Display` is the English text.
- **Plural forms**: a message about a count is a `MsgPlural`, written as
  `msg_plural!("{count} problem", "{count} problems")` and extracted with its plural (`msgid_plural`).
  `i18n::ntr(msg, n)` and `i18n::ntrf(msg, n, args)` pick the form for `n` by the rule of the catalog it comes from: the
  C expression of the header's `Plural-Forms` line, which the reader parses itself (gettext's operators and precedence,
  unsigned numbers, at most 1024 bytes and 32 levels deep). A catalog without a readable rule uses English's `n != 1`; a
  division by zero or an index past the forms gives the first form, as in gettext, and a translation without the form
  that is picked falls back to the next language, then to English (the singular for 1, else the plural). The count isn't
  filled in by itself: it is a placeholder like any other.
- **Showing**: `i18n::tr(msg)` gives the translation, or the English source string if there is none or `init` wasn't
  called, so tests stay in English without setup. A message with values in it names them (`{count} problems`), since
  `format!` needs a literal: `i18n::trf(msg, &[("count", &n)])` looks it up and fills them in, in one pass, so a value
  is never read for placeholders itself; `{{` and `}}` are literal braces, and a placeholder without a value stays as it
  is, so a faulty translation can't fail. Translators can move the placeholders. Search indexes both:
  `SearchIndex::new(registry, tr)` matches the translated and the English texts.
- **Pseudo-locale**: `i18n::pseudo::catalog` (core feature `test-support`) turns the template into a catalog in which
  every translation is its msgid with each ASCII letter swapped for an accented one, between `⟦` and `⟧`; placeholders,
  markup tags and entities stay. In it, a plain ASCII word in what is shown didn't go through a catalog, unless it is
  data. `crates/cli/tests/binary.rs` runs the commands that write nothing in that locale and fails on any English word
  that isn't data (what the user typed, the lines of the config, values, paths and versions from the `--json` output but
  not the texts of findings, names of programs, commands, the system's I/O errors). Another test there checks that
  `--json` keeps its English ids. `crates/gui/tests/gui_pseudo.rs` (needs a display; scripts/check-integration.sh runs
  it) opens every page, the welcome screen, the read-only banner and Sound, Bluetooth and Network against their
  stand-ins in that locale and fails on an English word in the labels, buttons, tooltips, placeholders and drop-down
  items of visible widgets, unless it is data (values, device and network names, the names of the XKB rules, paths,
  links, monospace lines of sway config); accessible labels can't be read back from GTK. `crates/core/tests/pot.rs`
  checks that every string marked in the sources is in the template, since xgettext skips some, such as those in the
  attributes of `glib::clone!`. `crates/core/tests/translated.rs` renders nested errors and the reasons a row is locked
  in that locale: every part is translated, so every part is marked and in the template, and the English text stays for
  logs.
