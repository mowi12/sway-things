_sway__things() {
    local i cur prev opts cmd
    COMPREPLY=()
    if [[ "${BASH_VERSINFO[0]}" -ge 4 ]]; then
        cur="$2"
    else
        cur="${COMP_WORDS[COMP_CWORD]}"
    fi
    prev="$3"
    cmd=""
    opts=""

    for i in "${COMP_WORDS[@]:0:COMP_CWORD}"
    do
        case "${cmd},${i}" in
            ",$1")
                cmd="sway__things"
                ;;
            sway__things,doctor)
                cmd="sway__things__subcmd__doctor"
                ;;
            sway__things,get)
                cmd="sway__things__subcmd__get"
                ;;
            sway__things,help)
                cmd="sway__things__subcmd__help"
                ;;
            sway__things,hook)
                cmd="sway__things__subcmd__hook"
                ;;
            sway__things,notifications)
                cmd="sway__things__subcmd__notifications"
                ;;
            sway__things,open)
                cmd="sway__things__subcmd__open"
                ;;
            sway__things,reset)
                cmd="sway__things__subcmd__reset"
                ;;
            sway__things,set)
                cmd="sway__things__subcmd__set"
                ;;
            sway__things,show-generated)
                cmd="sway__things__subcmd__show__subcmd__generated"
                ;;
            sway__things__subcmd__help,doctor)
                cmd="sway__things__subcmd__help__subcmd__doctor"
                ;;
            sway__things__subcmd__help,get)
                cmd="sway__things__subcmd__help__subcmd__get"
                ;;
            sway__things__subcmd__help,help)
                cmd="sway__things__subcmd__help__subcmd__help"
                ;;
            sway__things__subcmd__help,hook)
                cmd="sway__things__subcmd__help__subcmd__hook"
                ;;
            sway__things__subcmd__help,notifications)
                cmd="sway__things__subcmd__help__subcmd__notifications"
                ;;
            sway__things__subcmd__help,open)
                cmd="sway__things__subcmd__help__subcmd__open"
                ;;
            sway__things__subcmd__help,reset)
                cmd="sway__things__subcmd__help__subcmd__reset"
                ;;
            sway__things__subcmd__help,set)
                cmd="sway__things__subcmd__help__subcmd__set"
                ;;
            sway__things__subcmd__help,show-generated)
                cmd="sway__things__subcmd__help__subcmd__show__subcmd__generated"
                ;;
            sway__things__subcmd__help__subcmd__hook,displays)
                cmd="sway__things__subcmd__help__subcmd__hook__subcmd__displays"
                ;;
            sway__things__subcmd__help__subcmd__hook,gsettings)
                cmd="sway__things__subcmd__help__subcmd__hook__subcmd__gsettings"
                ;;
            sway__things__subcmd__help__subcmd__hook,mirror)
                cmd="sway__things__subcmd__help__subcmd__hook__subcmd__mirror"
                ;;
            sway__things__subcmd__help__subcmd__hook,notification-click)
                cmd="sway__things__subcmd__help__subcmd__hook__subcmd__notification__subcmd__click"
                ;;
            sway__things__subcmd__help__subcmd__hook,notification-daemon)
                cmd="sway__things__subcmd__help__subcmd__hook__subcmd__notification__subcmd__daemon"
                ;;
            sway__things__subcmd__help__subcmd__hook,quick)
                cmd="sway__things__subcmd__help__subcmd__hook__subcmd__quick"
                ;;
            sway__things__subcmd__help__subcmd__hook,workspaces)
                cmd="sway__things__subcmd__help__subcmd__hook__subcmd__workspaces"
                ;;
            sway__things__subcmd__help__subcmd__notifications,dnd)
                cmd="sway__things__subcmd__help__subcmd__notifications__subcmd__dnd"
                ;;
            sway__things__subcmd__help__subcmd__notifications,import-swaync)
                cmd="sway__things__subcmd__help__subcmd__notifications__subcmd__import__subcmd__swaync"
                ;;
            sway__things__subcmd__help__subcmd__notifications,status)
                cmd="sway__things__subcmd__help__subcmd__notifications__subcmd__status"
                ;;
            sway__things__subcmd__help__subcmd__notifications,test)
                cmd="sway__things__subcmd__help__subcmd__notifications__subcmd__test"
                ;;
            sway__things__subcmd__hook,displays)
                cmd="sway__things__subcmd__hook__subcmd__displays"
                ;;
            sway__things__subcmd__hook,gsettings)
                cmd="sway__things__subcmd__hook__subcmd__gsettings"
                ;;
            sway__things__subcmd__hook,help)
                cmd="sway__things__subcmd__hook__subcmd__help"
                ;;
            sway__things__subcmd__hook,mirror)
                cmd="sway__things__subcmd__hook__subcmd__mirror"
                ;;
            sway__things__subcmd__hook,notification-click)
                cmd="sway__things__subcmd__hook__subcmd__notification__subcmd__click"
                ;;
            sway__things__subcmd__hook,notification-daemon)
                cmd="sway__things__subcmd__hook__subcmd__notification__subcmd__daemon"
                ;;
            sway__things__subcmd__hook,quick)
                cmd="sway__things__subcmd__hook__subcmd__quick"
                ;;
            sway__things__subcmd__hook,workspaces)
                cmd="sway__things__subcmd__hook__subcmd__workspaces"
                ;;
            sway__things__subcmd__hook__subcmd__help,displays)
                cmd="sway__things__subcmd__hook__subcmd__help__subcmd__displays"
                ;;
            sway__things__subcmd__hook__subcmd__help,gsettings)
                cmd="sway__things__subcmd__hook__subcmd__help__subcmd__gsettings"
                ;;
            sway__things__subcmd__hook__subcmd__help,help)
                cmd="sway__things__subcmd__hook__subcmd__help__subcmd__help"
                ;;
            sway__things__subcmd__hook__subcmd__help,mirror)
                cmd="sway__things__subcmd__hook__subcmd__help__subcmd__mirror"
                ;;
            sway__things__subcmd__hook__subcmd__help,notification-click)
                cmd="sway__things__subcmd__hook__subcmd__help__subcmd__notification__subcmd__click"
                ;;
            sway__things__subcmd__hook__subcmd__help,notification-daemon)
                cmd="sway__things__subcmd__hook__subcmd__help__subcmd__notification__subcmd__daemon"
                ;;
            sway__things__subcmd__hook__subcmd__help,quick)
                cmd="sway__things__subcmd__hook__subcmd__help__subcmd__quick"
                ;;
            sway__things__subcmd__hook__subcmd__help,workspaces)
                cmd="sway__things__subcmd__hook__subcmd__help__subcmd__workspaces"
                ;;
            sway__things__subcmd__notifications,dnd)
                cmd="sway__things__subcmd__notifications__subcmd__dnd"
                ;;
            sway__things__subcmd__notifications,help)
                cmd="sway__things__subcmd__notifications__subcmd__help"
                ;;
            sway__things__subcmd__notifications,import-swaync)
                cmd="sway__things__subcmd__notifications__subcmd__import__subcmd__swaync"
                ;;
            sway__things__subcmd__notifications,status)
                cmd="sway__things__subcmd__notifications__subcmd__status"
                ;;
            sway__things__subcmd__notifications,test)
                cmd="sway__things__subcmd__notifications__subcmd__test"
                ;;
            sway__things__subcmd__notifications__subcmd__help,dnd)
                cmd="sway__things__subcmd__notifications__subcmd__help__subcmd__dnd"
                ;;
            sway__things__subcmd__notifications__subcmd__help,help)
                cmd="sway__things__subcmd__notifications__subcmd__help__subcmd__help"
                ;;
            sway__things__subcmd__notifications__subcmd__help,import-swaync)
                cmd="sway__things__subcmd__notifications__subcmd__help__subcmd__import__subcmd__swaync"
                ;;
            sway__things__subcmd__notifications__subcmd__help,status)
                cmd="sway__things__subcmd__notifications__subcmd__help__subcmd__status"
                ;;
            sway__things__subcmd__notifications__subcmd__help,test)
                cmd="sway__things__subcmd__notifications__subcmd__help__subcmd__test"
                ;;
            *)
                ;;
        esac
    done

    case "${cmd}" in
        sway__things)
            opts="-h -V --color --help --version get set reset show-generated doctor open notifications hook help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 1 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --color)
                    COMPREPLY=($(compgen -W "auto always never" -- "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__doctor)
            opts="-h --fix --overwrite --json --color --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --color)
                    COMPREPLY=($(compgen -W "auto always never" -- "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__get)
            opts="-h --device --json --color --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --device)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --color)
                    COMPREPLY=($(compgen -W "auto always never" -- "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__help)
            opts="get set reset show-generated doctor open notifications hook help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__help__subcmd__doctor)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__help__subcmd__get)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__help__subcmd__help)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__help__subcmd__hook)
            opts="gsettings displays workspaces mirror notification-click notification-daemon quick"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__help__subcmd__hook__subcmd__displays)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__help__subcmd__hook__subcmd__gsettings)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__help__subcmd__hook__subcmd__mirror)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__help__subcmd__hook__subcmd__notification__subcmd__click)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__help__subcmd__hook__subcmd__notification__subcmd__daemon)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__help__subcmd__hook__subcmd__quick)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__help__subcmd__hook__subcmd__workspaces)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__help__subcmd__notifications)
            opts="dnd status test import-swaync"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__help__subcmd__notifications__subcmd__dnd)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__help__subcmd__notifications__subcmd__import__subcmd__swaync)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__help__subcmd__notifications__subcmd__status)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__help__subcmd__notifications__subcmd__test)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__help__subcmd__open)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__help__subcmd__reset)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__help__subcmd__set)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__help__subcmd__show__subcmd__generated)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__hook)
            opts="-h --color --help gsettings displays workspaces mirror notification-click notification-daemon quick help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --color)
                    COMPREPLY=($(compgen -W "auto always never" -- "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__hook__subcmd__displays)
            opts="-h --color --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --color)
                    COMPREPLY=($(compgen -W "auto always never" -- "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__hook__subcmd__gsettings)
            opts="-h --color --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --color)
                    COMPREPLY=($(compgen -W "auto always never" -- "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__hook__subcmd__help)
            opts="gsettings displays workspaces mirror notification-click notification-daemon quick help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__hook__subcmd__help__subcmd__displays)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__hook__subcmd__help__subcmd__gsettings)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__hook__subcmd__help__subcmd__help)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__hook__subcmd__help__subcmd__mirror)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__hook__subcmd__help__subcmd__notification__subcmd__click)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__hook__subcmd__help__subcmd__notification__subcmd__daemon)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__hook__subcmd__help__subcmd__quick)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__hook__subcmd__help__subcmd__workspaces)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__hook__subcmd__mirror)
            opts="-h --color --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --color)
                    COMPREPLY=($(compgen -W "auto always never" -- "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__hook__subcmd__notification__subcmd__click)
            opts="-h --mako --color --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --mako)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --color)
                    COMPREPLY=($(compgen -W "auto always never" -- "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__hook__subcmd__notification__subcmd__daemon)
            opts="-h --color --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --color)
                    COMPREPLY=($(compgen -W "auto always never" -- "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__hook__subcmd__quick)
            opts="-h --color --help dnd wifi bluetooth sound microphone on off toggle state"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --color)
                    COMPREPLY=($(compgen -W "auto always never" -- "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__hook__subcmd__workspaces)
            opts="-h --color --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --color)
                    COMPREPLY=($(compgen -W "auto always never" -- "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__notifications)
            opts="-h --color --help dnd status test import-swaync help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --color)
                    COMPREPLY=($(compgen -W "auto always never" -- "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__notifications__subcmd__dnd)
            opts="-h --color --help on off toggle"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --color)
                    COMPREPLY=($(compgen -W "auto always never" -- "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__notifications__subcmd__help)
            opts="dnd status test import-swaync help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__notifications__subcmd__help__subcmd__dnd)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__notifications__subcmd__help__subcmd__help)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__notifications__subcmd__help__subcmd__import__subcmd__swaync)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__notifications__subcmd__help__subcmd__status)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__notifications__subcmd__help__subcmd__test)
            opts=""
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 4 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__notifications__subcmd__import__subcmd__swaync)
            opts="-h --color --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --color)
                    COMPREPLY=($(compgen -W "auto always never" -- "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__notifications__subcmd__status)
            opts="-h --color --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --color)
                    COMPREPLY=($(compgen -W "auto always never" -- "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__notifications__subcmd__test)
            opts="-h --color --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --color)
                    COMPREPLY=($(compgen -W "auto always never" -- "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__open)
            opts="-h --color --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --color)
                    COMPREPLY=($(compgen -W "auto always never" -- "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__reset)
            opts="-h --device --yes --overwrite --color --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --device)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --color)
                    COMPREPLY=($(compgen -W "auto always never" -- "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__set)
            opts="-h --device --host --shared --yes --overwrite --color --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --device)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --color)
                    COMPREPLY=($(compgen -W "auto always never" -- "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
        sway__subcmd__things__subcmd__show__subcmd__generated)
            opts="-h --color --help 10-input.conf 20-output.conf 30-appearance.conf 40-behavior.conf 80-hooks.conf 90-effects.conf"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --color)
                    COMPREPLY=($(compgen -W "auto always never" -- "${cur}"))
                    return 0
                    ;;
                *)
                    COMPREPLY=()
                    ;;
            esac
            COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
            return 0
            ;;
    esac
}

if [[ "${BASH_VERSINFO[0]}" -eq 4 && "${BASH_VERSINFO[1]}" -ge 4 || "${BASH_VERSINFO[0]}" -gt 4 ]]; then
    complete -F _sway__things -o nosort -o bashdefault -o default sway-things
else
    complete -F _sway__things -o bashdefault -o default sway-things
fi
