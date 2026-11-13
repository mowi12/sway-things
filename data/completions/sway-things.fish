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
complete -c sway-things -n "__fish_sway_things_needs_command" -f -a "reset" -d 'Remove the stored value of a setting and apply its default'
complete -c sway-things -n "__fish_sway_things_needs_command" -f -a "show-generated" -d 'Print the generated sway config files as they would be written, without writing them'
complete -c sway-things -n "__fish_sway_things_needs_command" -f -a "doctor" -d 'Check why settings might not take effect: the compositor, your sway config and its include line, the settings files, the generated files and the lines of your config that win'
complete -c sway-things -n "__fish_sway_things_needs_command" -f -a "open" -d 'Start the app, or bring its window up if it runs, at a page or a setting'
complete -c sway-things -n "__fish_sway_things_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c sway-things -n "__fish_sway_things_using_subcommand get" -l device -d 'The input device, by its identifier as `swaymsg -t get_inputs` shows it' -r
complete -c sway-things -n "__fish_sway_things_using_subcommand get" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand get" -l json -d 'Print JSON for scripts'
complete -c sway-things -n "__fish_sway_things_using_subcommand get" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand set" -l device -d 'The input device, by its identifier as `swaymsg -t get_inputs` shows it' -r
complete -c sway-things -n "__fish_sway_things_using_subcommand set" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand set" -l host -d 'Store the value for this machine only, in settings.d/<hostname>.toml'
complete -c sway-things -n "__fish_sway_things_using_subcommand set" -l shared -d 'Store the value in the shared settings.toml'
complete -c sway-things -n "__fish_sway_things_using_subcommand set" -l yes -d 'Change a setting that can lock you out without asking to keep it'
complete -c sway-things -n "__fish_sway_things_using_subcommand set" -l overwrite -d 'Replace generated files that were changed outside Sway Things'
complete -c sway-things -n "__fish_sway_things_using_subcommand set" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand reset" -l device -d 'The input device, by its identifier as `swaymsg -t get_inputs` shows it' -r
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
complete -c sway-things -n "__fish_sway_things_using_subcommand doctor" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand open" -l color -d 'Controls when to use color' -r -f -a "auto\t''
always\t''
never\t''"
complete -c sway-things -n "__fish_sway_things_using_subcommand open" -s h -l help -d 'Print help'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and not __fish_seen_subcommand_from get set reset show-generated doctor open help" -f -a "get" -d 'Show the value in effect and where it comes from; without a key, every stored setting'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and not __fish_seen_subcommand_from get set reset show-generated doctor open help" -f -a "set" -d 'Change a setting: live, in the settings files and in the generated sway config'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and not __fish_seen_subcommand_from get set reset show-generated doctor open help" -f -a "reset" -d 'Remove the stored value of a setting and apply its default'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and not __fish_seen_subcommand_from get set reset show-generated doctor open help" -f -a "show-generated" -d 'Print the generated sway config files as they would be written, without writing them'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and not __fish_seen_subcommand_from get set reset show-generated doctor open help" -f -a "doctor" -d 'Check why settings might not take effect: the compositor, your sway config and its include line, the settings files, the generated files and the lines of your config that win'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and not __fish_seen_subcommand_from get set reset show-generated doctor open help" -f -a "open" -d 'Start the app, or bring its window up if it runs, at a page or a setting'
complete -c sway-things -n "__fish_sway_things_using_subcommand help; and not __fish_seen_subcommand_from get set reset show-generated doctor open help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
