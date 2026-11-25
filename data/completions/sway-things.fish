# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_sway_things_global_optspecs
    string join \n color= h/help V/version
end

function __fish_sway_things_needs_command
    # Figure out if the current invocation already has a command.
    set -l cmd (commandline -opc)
    set -e cmd[1]
    argparse -s (__fish_sway_things_global_optspecs) -- $cmd 2>/dev/null
    or return
    if set -q argv[1]
        # Also print the command, so this can be used to figure out what it is.
        echo $argv[1]
        return 1
    end
    return 0
end

function __fish_sway_things_using_subcommand
    set -l cmd (__fish_sway_things_needs_command)
    test -z "$cmd"
    and return 1
    contains -- $cmd[1] $argv
end

complete -c sway-things -n "__fish_sway_things_needs_command" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_needs_command" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_needs_command" -s V -l version -d 'Print version'
complete -c sway-things -n "__fish_sway_things_needs_command" -f -a "get" -d 'Show the value in effect and where it comes from; without a key, every stored setting'
complete -c sway-things -n "__fish_sway_things_needs_command" -f -a "set" -d 'Change a setting: live, in the settings files and in the generated sway config'
complete -c sway-things -n "__fish_sway_things_needs_command" -f -a "reset" -d 'Remove the stored value of a setting and apply the value of your config, else its default'
complete -c sway-things -n "__fish_sway_things_needs_command" -f -a "show-generated" -d 'Print the generated sway config files as they would be written, without writing them'
complete -c sway-things -n "__fish_sway_things_needs_command" -f -a "doctor" -d 'Check why settings might not take effect: the compositor, your sway config and its include line, the settings files, the generated files and the lines of your config that win'
complete -c sway-things -n "__fish_sway_things_needs_command" -f -a "open" -d 'Start the app, or bring its window up if it runs, at a page or a setting'
complete -c sway-things -n "__fish_sway_things_needs_command" -f -a "notifications" -d 'Do Not Disturb, the notification daemon that runs, and a test notification'
complete -c sway-things -n "__fish_sway_things_needs_command" -f -a "hook" -d 'Run a hook; the generated sway config runs these'
complete -c sway-things -n "__fish_sway_things_needs_command" -f -a "screenshot" -d 'Take a screenshot with grim, and save and copy it as the Screenshots & Clipboard page says; the screenshot keys run this'
complete -c sway-things -n "__fish_sway_things_needs_command" -f -a "record" -d 'Start a recording of the screen with wf-recorder or wl-screenrec, or stop the one that runs so its file is complete; the recording key runs this'
complete -c sway-things -n "__fish_sway_things_needs_command" -f -a "clipboard" -d 'Pick an entry of the clipboard history, or clear it'
complete -c sway-things -n "__fish_sway_things_needs_command" -f -a "wallpaper" -d 'Change the picture of a wallpaper collection'
complete -c sway-things -n "__fish_sway_things_needs_command" -f -a "brightness" -d 'Print or change the brightness of the screen\'s backlight, through logind'
complete -c sway-things -n "__fish_sway_things_needs_command" -f -a "night-light" -d 'Switch the night light on or off, as the Brightness & Night Light page does: stores the switch and starts or ends its tool (wlsunset or gammastep)'
complete -c sway-things -n "__fish_sway_things_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c sway-things -n "__fish_sway_things_using_subcommand get" -l device -d 'The input device or display: an input device by its identifier as `swaymsg -t get_inputs` shows it, a display by its make, model and serial as `swaymsg -t get_outputs` shows them, or in a sway session by its name' -r
complete -c sway-things -n "__fish_sway_things_using_subcommand get" -l collection -d 'The wallpaper collection, by its name' -r
complete -c sway-things -n "__fish_sway_things_using_subcommand get" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand get" -l json -d 'Print JSON for scripts'
complete -c sway-things -n "__fish_sway_things_using_subcommand get" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand set" -l device -d 'The input device or display: an input device by its identifier as `swaymsg -t get_inputs` shows it, a display by its make, model and serial as `swaymsg -t get_outputs` shows them, or in a sway session by its name' -r
complete -c sway-things -n "__fish_sway_things_using_subcommand set" -l collection -d 'The wallpaper collection, by its name' -r
complete -c sway-things -n "__fish_sway_things_using_subcommand set" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand set" -l host -d 'Store the value for this machine only, in settings.d/<hostname>.toml'
complete -c sway-things -n "__fish_sway_things_using_subcommand set" -l shared -d 'Store the value in the shared settings.toml'
complete -c sway-things -n "__fish_sway_things_using_subcommand set" -l yes -d 'Change a setting that can lock you out without asking to keep it'
complete -c sway-things -n "__fish_sway_things_using_subcommand set" -l overwrite -d 'Replace generated files that were changed outside Sway Things'
complete -c sway-things -n "__fish_sway_things_using_subcommand set" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand reset" -l device -d 'The input device or display: an input device by its identifier as `swaymsg -t get_inputs` shows it, a display by its make, model and serial as `swaymsg -t get_outputs` shows them, or in a sway session by its name' -r
complete -c sway-things -n "__fish_sway_things_using_subcommand reset" -l collection -d 'The wallpaper collection, by its name' -r
complete -c sway-things -n "__fish_sway_things_using_subcommand reset" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand reset" -l yes -d 'Reset a setting that can lock you out without asking to keep it'
complete -c sway-things -n "__fish_sway_things_using_subcommand reset" -l overwrite -d 'Replace generated files that were changed outside Sway Things'
complete -c sway-things -n "__fish_sway_things_using_subcommand reset" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand show-generated" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand show-generated" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand doctor" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand doctor" -l fix -d 'Write the generated files that are missing or out of date. Never touches your own files'
complete -c sway-things -n "__fish_sway_things_using_subcommand doctor" -l overwrite -d 'With --fix, also replace generated files that were changed outside Sway Things'
complete -c sway-things -n "__fish_sway_things_using_subcommand doctor" -l json -d 'Print JSON for scripts'
complete -c sway-things -n "__fish_sway_things_using_subcommand doctor" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c sway-things -n "__fish_sway_things_using_subcommand open" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand open" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand notifications; and not __fish_seen_subcommand_from dnd status test import-swaync help" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand notifications; and not __fish_seen_subcommand_from dnd status test import-swaync help" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand notifications; and not __fish_seen_subcommand_from dnd status test import-swaync help" -f -a "dnd" -d 'Show whether Do Not Disturb is on, or switch it: for mako, swaync and dunst'
complete -c sway-things -n "__fish_sway_things_using_subcommand notifications; and not __fish_seen_subcommand_from dnd status test import-swaync help" -f -a "status" -d 'Show which notification daemon runs and whether Do Not Disturb is on'
complete -c sway-things -n "__fish_sway_things_using_subcommand notifications; and not __fish_seen_subcommand_from dnd status test import-swaync help" -f -a "test" -d 'Send a test notification through the daemon that runs'
complete -c sway-things -n "__fish_sway_things_using_subcommand notifications; and not __fish_seen_subcommand_from dnd status test import-swaync help" -f -a "import-swaync" -d 'Read a swaync config.json (yours, by default) into the settings of the Notifications page as a starting point; your file isn\'t changed'
complete -c sway-things -n "__fish_sway_things_using_subcommand notifications; and not __fish_seen_subcommand_from dnd status test import-swaync help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c sway-things -n "__fish_sway_things_using_subcommand notifications; and __fish_seen_subcommand_from dnd" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand notifications; and __fish_seen_subcommand_from dnd" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c sway-things -n "__fish_sway_things_using_subcommand notifications; and __fish_seen_subcommand_from status" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand notifications; and __fish_seen_subcommand_from status" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand notifications; and __fish_seen_subcommand_from test" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand notifications; and __fish_seen_subcommand_from test" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand notifications; and __fish_seen_subcommand_from import-swaync" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand notifications; and __fish_seen_subcommand_from import-swaync" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand notifications; and __fish_seen_subcommand_from help" -f -a "dnd" -d 'Show whether Do Not Disturb is on, or switch it: for mako, swaync and dunst'
complete -c sway-things -n "__fish_sway_things_using_subcommand notifications; and __fish_seen_subcommand_from help" -f -a "status" -d 'Show which notification daemon runs and whether Do Not Disturb is on'
complete -c sway-things -n "__fish_sway_things_using_subcommand notifications; and __fish_seen_subcommand_from help" -f -a "test" -d 'Send a test notification through the daemon that runs'
complete -c sway-things -n "__fish_sway_things_using_subcommand notifications; and __fish_seen_subcommand_from help" -f -a "import-swaync" -d 'Read a swaync config.json (yours, by default) into the settings of the Notifications page as a starting point; your file isn\'t changed'
complete -c sway-things -n "__fish_sway_things_using_subcommand notifications; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and not __fish_seen_subcommand_from gsettings displays workspaces mirror new-display wallpaper clipboard night-light launch autostart notification-click notification-daemon quick help" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and not __fish_seen_subcommand_from gsettings displays workspaces mirror new-display wallpaper clipboard night-light launch autostart notification-click notification-daemon quick help" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and not __fish_seen_subcommand_from gsettings displays workspaces mirror new-display wallpaper clipboard night-light launch autostart notification-click notification-daemon quick help" -f -a "gsettings" -d 'Copy the stored settings that GTK apps read from GSettings (cursor theme and size) to GSettings; run at login and at each reload of sway'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and not __fish_seen_subcommand_from gsettings displays workspaces mirror new-display wallpaper clipboard night-light launch autostart notification-click notification-daemon quick help" -f -a "displays" -d 'Switch a display on if none is on (and have kanshi apply its profile again, if it runs): run at every start and reload of sway while Sway Things stores anything for a display, so it never leaves you without one'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and not __fish_seen_subcommand_from gsettings displays workspaces mirror new-display wallpaper clipboard night-light launch autostart notification-click notification-daemon quick help" -f -a "workspaces" -d 'Put the workspaces on the displays connected now as the Workspaces page says: run at every start and reload of sway while a rule places them, and by kanshi when it applied a profile'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and not __fish_seen_subcommand_from gsettings displays workspaces mirror new-display wallpaper clipboard night-light launch autostart notification-click notification-daemon quick help" -f -a "mirror" -d 'Show displays on others as remembered for the displays connected now (wl-mirror), and end the mirrors of other combinations: run by kanshi when it applied a profile, and from a key binding of yours to bring back a picture that went'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and not __fish_seen_subcommand_from gsettings displays workspaces mirror new-display wallpaper clipboard night-light launch autostart notification-click notification-daemon quick help" -f -a "new-display" -d 'Ask once whether a display plugged in to a combination set up on the Displays page shows the same picture or extends the desktop, and remember the answer: run by kanshi when one display more than such a combination is connected'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and not __fish_seen_subcommand_from gsettings displays workspaces mirror new-display wallpaper clipboard night-light launch autostart notification-click notification-daemon quick help" -f -a "wallpaper" -d 'Show the next picture of each wallpaper collection whose time has come, and start the timers of those that change daily or after an interval: run when sway starts while a display shows a collection'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and not __fish_seen_subcommand_from gsettings displays workspaces mirror new-display wallpaper clipboard night-light launch autostart notification-click notification-daemon quick help" -f -a "clipboard" -d 'Start the watcher of the clipboard history (wl-paste --watch cliphist store) if the Screenshots & Clipboard page has the history on, and end it if not: run at every start and reload of sway while it is on'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and not __fish_seen_subcommand_from gsettings displays workspaces mirror new-display wallpaper clipboard night-light launch autostart notification-click notification-daemon quick help" -f -a "night-light" -d 'Start the night light\'s tool (wlsunset or gammastep) as the Brightness & Night Light page says, and end it if the night light is off: run at every start and reload of sway while it is on. A night light your config starts itself, or that runs already, is left alone'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and not __fish_seen_subcommand_from gsettings displays workspaces mirror new-display wallpaper clipboard night-light launch autostart notification-click notification-daemon quick help" -f -a "launch" -d 'Start an installed app by its desktop file ID, as its desktop entry says (in a terminal if it asks for one): the autostart entries of Sway Things that are apps run this'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and not __fish_seen_subcommand_from gsettings displays workspaces mirror new-display wallpaper clipboard night-light launch autostart notification-click notification-daemon quick help" -f -a "autostart" -d 'Hand the session\'s variables to the systemd user session and start the XDG autostart entries through it (xdg-desktop-autostart.target): run at login while Sway Things starts them with systemd'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and not __fish_seen_subcommand_from gsettings displays workspaces mirror new-display wallpaper clipboard night-light launch autostart notification-click notification-daemon quick help" -f -a "notification-click" -d 'Bring the window of a clicked notification\'s app to the front: run by swaync (as a script, with the notification in its environment) and mako (with --mako) while the Notifications page says so'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and not __fish_seen_subcommand_from gsettings displays workspaces mirror new-display wallpaper clipboard night-light launch autostart notification-click notification-daemon quick help" -f -a "notification-daemon" -d 'Start the notification daemon chosen on the Notifications page unless one runs: run at every start and reload of sway while one is chosen'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and not __fish_seen_subcommand_from gsettings displays workspaces mirror new-display wallpaper clipboard night-light launch autostart notification-click notification-daemon quick help" -f -a "quick" -d 'Switch Do Not Disturb, Wi-Fi, Bluetooth, the sound or the microphone: run by the buttons of swaync\'s panel'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and not __fish_seen_subcommand_from gsettings displays workspaces mirror new-display wallpaper clipboard night-light launch autostart notification-click notification-daemon quick help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from gsettings" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from gsettings" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from displays" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from displays" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from workspaces" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from workspaces" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from mirror" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from mirror" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from new-display" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from new-display" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from wallpaper" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from wallpaper" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from clipboard" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from clipboard" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from night-light" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from night-light" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from launch" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from launch" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from autostart" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from autostart" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from notification-click" -l mako -d 'The id of mako\'s notification; mako runs the hook with it' -r
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from notification-click" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from notification-click" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from notification-daemon" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from notification-daemon" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from quick" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from quick" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from help" -f -a "gsettings" -d 'Copy the stored settings that GTK apps read from GSettings (cursor theme and size) to GSettings; run at login and at each reload of sway'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from help" -f -a "displays" -d 'Switch a display on if none is on (and have kanshi apply its profile again, if it runs): run at every start and reload of sway while Sway Things stores anything for a display, so it never leaves you without one'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from help" -f -a "workspaces" -d 'Put the workspaces on the displays connected now as the Workspaces page says: run at every start and reload of sway while a rule places them, and by kanshi when it applied a profile'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from help" -f -a "mirror" -d 'Show displays on others as remembered for the displays connected now (wl-mirror), and end the mirrors of other combinations: run by kanshi when it applied a profile, and from a key binding of yours to bring back a picture that went'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from help" -f -a "new-display" -d 'Ask once whether a display plugged in to a combination set up on the Displays page shows the same picture or extends the desktop, and remember the answer: run by kanshi when one display more than such a combination is connected'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from help" -f -a "wallpaper" -d 'Show the next picture of each wallpaper collection whose time has come, and start the timers of those that change daily or after an interval: run when sway starts while a display shows a collection'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from help" -f -a "clipboard" -d 'Start the watcher of the clipboard history (wl-paste --watch cliphist store) if the Screenshots & Clipboard page has the history on, and end it if not: run at every start and reload of sway while it is on'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from help" -f -a "night-light" -d 'Start the night light\'s tool (wlsunset or gammastep) as the Brightness & Night Light page says, and end it if the night light is off: run at every start and reload of sway while it is on. A night light your config starts itself, or that runs already, is left alone'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from help" -f -a "launch" -d 'Start an installed app by its desktop file ID, as its desktop entry says (in a terminal if it asks for one): the autostart entries of Sway Things that are apps run this'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from help" -f -a "autostart" -d 'Hand the session\'s variables to the systemd user session and start the XDG autostart entries through it (xdg-desktop-autostart.target): run at login while Sway Things starts them with systemd'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from help" -f -a "notification-click" -d 'Bring the window of a clicked notification\'s app to the front: run by swaync (as a script, with the notification in its environment) and mako (with --mako) while the Notifications page says so'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from help" -f -a "notification-daemon" -d 'Start the notification daemon chosen on the Notifications page unless one runs: run at every start and reload of sway while one is chosen'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from help" -f -a "quick" -d 'Switch Do Not Disturb, Wi-Fi, Bluetooth, the sound or the microphone: run by the buttons of swaync\'s panel'
complete -c sway-things -n "__fish_sway_things_using_subcommand hook; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c sway-things -n "__fish_sway_things_using_subcommand screenshot" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand screenshot" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c sway-things -n "__fish_sway_things_using_subcommand record" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand record" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c sway-things -n "__fish_sway_things_using_subcommand clipboard; and not __fish_seen_subcommand_from pick clear help" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand clipboard; and not __fish_seen_subcommand_from pick clear help" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand clipboard; and not __fish_seen_subcommand_from pick clear help" -f -a "pick" -d 'Show the history in the launcher that is installed (fuzzel, wofi, rofi, tofi or bemenu) and copy the entry you pick again; the history key runs this'
complete -c sway-things -n "__fish_sway_things_using_subcommand clipboard; and not __fish_seen_subcommand_from pick clear help" -f -a "clear" -d 'Remove every entry of the history'
complete -c sway-things -n "__fish_sway_things_using_subcommand clipboard; and not __fish_seen_subcommand_from pick clear help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c sway-things -n "__fish_sway_things_using_subcommand clipboard; and __fish_seen_subcommand_from pick" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand clipboard; and __fish_seen_subcommand_from pick" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand clipboard; and __fish_seen_subcommand_from clear" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand clipboard; and __fish_seen_subcommand_from clear" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand clipboard; and __fish_seen_subcommand_from help" -f -a "pick" -d 'Show the history in the launcher that is installed (fuzzel, wofi, rofi, tofi or bemenu) and copy the entry you pick again; the history key runs this'
complete -c sway-things -n "__fish_sway_things_using_subcommand clipboard; and __fish_seen_subcommand_from help" -f -a "clear" -d 'Remove every entry of the history'
complete -c sway-things -n "__fish_sway_things_using_subcommand clipboard; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c sway-things -n "__fish_sway_things_using_subcommand wallpaper; and not __fish_seen_subcommand_from next help" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand wallpaper; and not __fish_seen_subcommand_from next help" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c sway-things -n "__fish_sway_things_using_subcommand wallpaper; and not __fish_seen_subcommand_from next help" -f -a "next" -d 'Show the next picture of the collection'
complete -c sway-things -n "__fish_sway_things_using_subcommand wallpaper; and not __fish_seen_subcommand_from next help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c sway-things -n "__fish_sway_things_using_subcommand wallpaper; and __fish_seen_subcommand_from next" -l device -d 'The display, as for `get`; without one, the collection of every display' -r
complete -c sway-things -n "__fish_sway_things_using_subcommand wallpaper; and __fish_seen_subcommand_from next" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand wallpaper; and __fish_seen_subcommand_from next" -l due -d 'Only if its time has come; without a sway session, stop the timer instead'
complete -c sway-things -n "__fish_sway_things_using_subcommand wallpaper; and __fish_seen_subcommand_from next" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand wallpaper; and __fish_seen_subcommand_from help" -f -a "next" -d 'Show the next picture of the collection'
complete -c sway-things -n "__fish_sway_things_using_subcommand wallpaper; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and not __fish_seen_subcommand_from get set up down help" -l device -d 'The backlight or keyboard light by its name in /sys/class/backlight or /sys/class/leds, e.g. `intel_backlight` or `tpacpi::kbd_backlight`' -r
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and not __fish_seen_subcommand_from get set up down help" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and not __fish_seen_subcommand_from get set up down help" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and not __fish_seen_subcommand_from get set up down help" -f -a "get" -d 'Print the brightness in percent'
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and not __fish_seen_subcommand_from get set up down help" -f -a "set" -d 'Set the brightness, in percent: 50% or 50'
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and not __fish_seen_subcommand_from get set up down help" -f -a "up" -d 'Make it brighter by 5 %'
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and not __fish_seen_subcommand_from get set up down help" -f -a "down" -d 'Make it darker by 5 %, not below 1 % for a backlight'
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and not __fish_seen_subcommand_from get set up down help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and __fish_seen_subcommand_from get" -l device -d 'The backlight or keyboard light by its name in /sys/class/backlight or /sys/class/leds, e.g. `intel_backlight` or `tpacpi::kbd_backlight`' -r
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and __fish_seen_subcommand_from get" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and __fish_seen_subcommand_from get" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and __fish_seen_subcommand_from set" -l device -d 'The backlight or keyboard light by its name in /sys/class/backlight or /sys/class/leds, e.g. `intel_backlight` or `tpacpi::kbd_backlight`' -r
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and __fish_seen_subcommand_from set" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and __fish_seen_subcommand_from set" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and __fish_seen_subcommand_from up" -l device -d 'The backlight or keyboard light by its name in /sys/class/backlight or /sys/class/leds, e.g. `intel_backlight` or `tpacpi::kbd_backlight`' -r
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and __fish_seen_subcommand_from up" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and __fish_seen_subcommand_from up" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and __fish_seen_subcommand_from down" -l device -d 'The backlight or keyboard light by its name in /sys/class/backlight or /sys/class/leds, e.g. `intel_backlight` or `tpacpi::kbd_backlight`' -r
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and __fish_seen_subcommand_from down" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and __fish_seen_subcommand_from down" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and __fish_seen_subcommand_from help" -f -a "get" -d 'Print the brightness in percent'
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and __fish_seen_subcommand_from help" -f -a "set" -d 'Set the brightness, in percent: 50% or 50'
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and __fish_seen_subcommand_from help" -f -a "up" -d 'Make it brighter by 5 %'
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and __fish_seen_subcommand_from help" -f -a "down" -d 'Make it darker by 5 %, not below 1 % for a backlight'
complete -c sway-things -n "__fish_sway_things_using_subcommand brightness; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c sway-things -n "__fish_sway_things_using_subcommand night-light" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand night-light" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and not __fish_seen_subcommand_from get set reset show-generated doctor open notifications hook screenshot record clipboard wallpaper brightness night-light help" -f -a "get" -d 'Show the value in effect and where it comes from; without a key, every stored setting'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and not __fish_seen_subcommand_from get set reset show-generated doctor open notifications hook screenshot record clipboard wallpaper brightness night-light help" -f -a "set" -d 'Change a setting: live, in the settings files and in the generated sway config'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and not __fish_seen_subcommand_from get set reset show-generated doctor open notifications hook screenshot record clipboard wallpaper brightness night-light help" -f -a "reset" -d 'Remove the stored value of a setting and apply the value of your config, else its default'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and not __fish_seen_subcommand_from get set reset show-generated doctor open notifications hook screenshot record clipboard wallpaper brightness night-light help" -f -a "show-generated" -d 'Print the generated sway config files as they would be written, without writing them'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and not __fish_seen_subcommand_from get set reset show-generated doctor open notifications hook screenshot record clipboard wallpaper brightness night-light help" -f -a "doctor" -d 'Check why settings might not take effect: the compositor, your sway config and its include line, the settings files, the generated files and the lines of your config that win'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and not __fish_seen_subcommand_from get set reset show-generated doctor open notifications hook screenshot record clipboard wallpaper brightness night-light help" -f -a "open" -d 'Start the app, or bring its window up if it runs, at a page or a setting'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and not __fish_seen_subcommand_from get set reset show-generated doctor open notifications hook screenshot record clipboard wallpaper brightness night-light help" -f -a "notifications" -d 'Do Not Disturb, the notification daemon that runs, and a test notification'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and not __fish_seen_subcommand_from get set reset show-generated doctor open notifications hook screenshot record clipboard wallpaper brightness night-light help" -f -a "hook" -d 'Run a hook; the generated sway config runs these'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and not __fish_seen_subcommand_from get set reset show-generated doctor open notifications hook screenshot record clipboard wallpaper brightness night-light help" -f -a "screenshot" -d 'Take a screenshot with grim, and save and copy it as the Screenshots & Clipboard page says; the screenshot keys run this'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and not __fish_seen_subcommand_from get set reset show-generated doctor open notifications hook screenshot record clipboard wallpaper brightness night-light help" -f -a "record" -d 'Start a recording of the screen with wf-recorder or wl-screenrec, or stop the one that runs so its file is complete; the recording key runs this'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and not __fish_seen_subcommand_from get set reset show-generated doctor open notifications hook screenshot record clipboard wallpaper brightness night-light help" -f -a "clipboard" -d 'Pick an entry of the clipboard history, or clear it'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and not __fish_seen_subcommand_from get set reset show-generated doctor open notifications hook screenshot record clipboard wallpaper brightness night-light help" -f -a "wallpaper" -d 'Change the picture of a wallpaper collection'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and not __fish_seen_subcommand_from get set reset show-generated doctor open notifications hook screenshot record clipboard wallpaper brightness night-light help" -f -a "brightness" -d 'Print or change the brightness of the screen\'s backlight, through logind'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and not __fish_seen_subcommand_from get set reset show-generated doctor open notifications hook screenshot record clipboard wallpaper brightness night-light help" -f -a "night-light" -d 'Switch the night light on or off, as the Brightness & Night Light page does: stores the switch and starts or ends its tool (wlsunset or gammastep)'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and not __fish_seen_subcommand_from get set reset show-generated doctor open notifications hook screenshot record clipboard wallpaper brightness night-light help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from notifications" -f -a "dnd" -d 'Show whether Do Not Disturb is on, or switch it: for mako, swaync and dunst'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from notifications" -f -a "status" -d 'Show which notification daemon runs and whether Do Not Disturb is on'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from notifications" -f -a "test" -d 'Send a test notification through the daemon that runs'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from notifications" -f -a "import-swaync" -d 'Read a swaync config.json (yours, by default) into the settings of the Notifications page as a starting point; your file isn\'t changed'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from hook" -f -a "gsettings" -d 'Copy the stored settings that GTK apps read from GSettings (cursor theme and size) to GSettings; run at login and at each reload of sway'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from hook" -f -a "displays" -d 'Switch a display on if none is on (and have kanshi apply its profile again, if it runs): run at every start and reload of sway while Sway Things stores anything for a display, so it never leaves you without one'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from hook" -f -a "workspaces" -d 'Put the workspaces on the displays connected now as the Workspaces page says: run at every start and reload of sway while a rule places them, and by kanshi when it applied a profile'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from hook" -f -a "mirror" -d 'Show displays on others as remembered for the displays connected now (wl-mirror), and end the mirrors of other combinations: run by kanshi when it applied a profile, and from a key binding of yours to bring back a picture that went'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from hook" -f -a "new-display" -d 'Ask once whether a display plugged in to a combination set up on the Displays page shows the same picture or extends the desktop, and remember the answer: run by kanshi when one display more than such a combination is connected'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from hook" -f -a "wallpaper" -d 'Show the next picture of each wallpaper collection whose time has come, and start the timers of those that change daily or after an interval: run when sway starts while a display shows a collection'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from hook" -f -a "clipboard" -d 'Start the watcher of the clipboard history (wl-paste --watch cliphist store) if the Screenshots & Clipboard page has the history on, and end it if not: run at every start and reload of sway while it is on'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from hook" -f -a "night-light" -d 'Start the night light\'s tool (wlsunset or gammastep) as the Brightness & Night Light page says, and end it if the night light is off: run at every start and reload of sway while it is on. A night light your config starts itself, or that runs already, is left alone'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from hook" -f -a "launch" -d 'Start an installed app by its desktop file ID, as its desktop entry says (in a terminal if it asks for one): the autostart entries of Sway Things that are apps run this'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from hook" -f -a "autostart" -d 'Hand the session\'s variables to the systemd user session and start the XDG autostart entries through it (xdg-desktop-autostart.target): run at login while Sway Things starts them with systemd'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from hook" -f -a "notification-click" -d 'Bring the window of a clicked notification\'s app to the front: run by swaync (as a script, with the notification in its environment) and mako (with --mako) while the Notifications page says so'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from hook" -f -a "notification-daemon" -d 'Start the notification daemon chosen on the Notifications page unless one runs: run at every start and reload of sway while one is chosen'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from hook" -f -a "quick" -d 'Switch Do Not Disturb, Wi-Fi, Bluetooth, the sound or the microphone: run by the buttons of swaync\'s panel'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from clipboard" -f -a "pick" -d 'Show the history in the launcher that is installed (fuzzel, wofi, rofi, tofi or bemenu) and copy the entry you pick again; the history key runs this'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from clipboard" -f -a "clear" -d 'Remove every entry of the history'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from wallpaper" -f -a "next" -d 'Show the next picture of the collection'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from brightness" -f -a "get" -d 'Print the brightness in percent'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from brightness" -f -a "set" -d 'Set the brightness, in percent: 50% or 50'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from brightness" -f -a "up" -d 'Make it brighter by 5 %'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and __fish_seen_subcommand_from brightness" -f -a "down" -d 'Make it darker by 5 %, not below 1 % for a backlight'
