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
            sway__things,clipboard)
                cmd="sway__things__subcmd__clipboard"
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
            sway__things,open)
                cmd="sway__things__subcmd__open"
                ;;
            sway__things,record)
                cmd="sway__things__subcmd__record"
                ;;
            sway__things,reset)
                cmd="sway__things__subcmd__reset"
                ;;
            sway__things,screenshot)
                cmd="sway__things__subcmd__screenshot"
                ;;
            sway__things,set)
                cmd="sway__things__subcmd__set"
                ;;
            sway__things,show-generated)
                cmd="sway__things__subcmd__show__subcmd__generated"
                ;;
            sway__things,wallpaper)
                cmd="sway__things__subcmd__wallpaper"
                ;;
            sway__things__subcmd__clipboard,clear)
                cmd="sway__things__subcmd__clipboard__subcmd__clear"
                ;;
            sway__things__subcmd__clipboard,help)
                cmd="sway__things__subcmd__clipboard__subcmd__help"
                ;;
            sway__things__subcmd__clipboard,pick)
                cmd="sway__things__subcmd__clipboard__subcmd__pick"
                ;;
            sway__things__subcmd__clipboard__subcmd__help,clear)
                cmd="sway__things__subcmd__clipboard__subcmd__help__subcmd__clear"
                ;;
            sway__things__subcmd__clipboard__subcmd__help,help)
                cmd="sway__things__subcmd__clipboard__subcmd__help__subcmd__help"
                ;;
            sway__things__subcmd__clipboard__subcmd__help,pick)
                cmd="sway__things__subcmd__clipboard__subcmd__help__subcmd__pick"
                ;;
            sway__things__subcmd__help,clipboard)
                cmd="sway__things__subcmd__help__subcmd__clipboard"
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
            sway__things__subcmd__help,open)
                cmd="sway__things__subcmd__help__subcmd__open"
                ;;
            sway__things__subcmd__help,record)
                cmd="sway__things__subcmd__help__subcmd__record"
                ;;
            sway__things__subcmd__help,reset)
                cmd="sway__things__subcmd__help__subcmd__reset"
                ;;
            sway__things__subcmd__help,screenshot)
                cmd="sway__things__subcmd__help__subcmd__screenshot"
                ;;
            sway__things__subcmd__help,set)
                cmd="sway__things__subcmd__help__subcmd__set"
                ;;
            sway__things__subcmd__help,show-generated)
                cmd="sway__things__subcmd__help__subcmd__show__subcmd__generated"
                ;;
            sway__things__subcmd__help,wallpaper)
                cmd="sway__things__subcmd__help__subcmd__wallpaper"
                ;;
            sway__things__subcmd__help__subcmd__clipboard,clear)
                cmd="sway__things__subcmd__help__subcmd__clipboard__subcmd__clear"
                ;;
            sway__things__subcmd__help__subcmd__clipboard,pick)
                cmd="sway__things__subcmd__help__subcmd__clipboard__subcmd__pick"
                ;;
            sway__things__subcmd__help__subcmd__hook,autostart)
                cmd="sway__things__subcmd__help__subcmd__hook__subcmd__autostart"
                ;;
            sway__things__subcmd__help__subcmd__hook,clipboard)
                cmd="sway__things__subcmd__help__subcmd__hook__subcmd__clipboard"
                ;;
            sway__things__subcmd__help__subcmd__hook,displays)
                cmd="sway__things__subcmd__help__subcmd__hook__subcmd__displays"
                ;;
            sway__things__subcmd__help__subcmd__hook,gsettings)
                cmd="sway__things__subcmd__help__subcmd__hook__subcmd__gsettings"
                ;;
            sway__things__subcmd__help__subcmd__hook,launch)
                cmd="sway__things__subcmd__help__subcmd__hook__subcmd__launch"
                ;;
            sway__things__subcmd__help__subcmd__hook,mirror)
                cmd="sway__things__subcmd__help__subcmd__hook__subcmd__mirror"
                ;;
            sway__things__subcmd__help__subcmd__hook,new-display)
                cmd="sway__things__subcmd__help__subcmd__hook__subcmd__new__subcmd__display"
                ;;
            sway__things__subcmd__help__subcmd__hook,wallpaper)
                cmd="sway__things__subcmd__help__subcmd__hook__subcmd__wallpaper"
                ;;
            sway__things__subcmd__help__subcmd__hook,workspaces)
                cmd="sway__things__subcmd__help__subcmd__hook__subcmd__workspaces"
                ;;
            sway__things__subcmd__help__subcmd__wallpaper,next)
                cmd="sway__things__subcmd__help__subcmd__wallpaper__subcmd__next"
                ;;
            sway__things__subcmd__hook,autostart)
                cmd="sway__things__subcmd__hook__subcmd__autostart"
                ;;
            sway__things__subcmd__hook,clipboard)
                cmd="sway__things__subcmd__hook__subcmd__clipboard"
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
            sway__things__subcmd__hook,launch)
                cmd="sway__things__subcmd__hook__subcmd__launch"
                ;;
            sway__things__subcmd__hook,mirror)
                cmd="sway__things__subcmd__hook__subcmd__mirror"
                ;;
            sway__things__subcmd__hook,new-display)
                cmd="sway__things__subcmd__hook__subcmd__new__subcmd__display"
                ;;
            sway__things__subcmd__hook,wallpaper)
                cmd="sway__things__subcmd__hook__subcmd__wallpaper"
                ;;
            sway__things__subcmd__hook,workspaces)
                cmd="sway__things__subcmd__hook__subcmd__workspaces"
                ;;
            sway__things__subcmd__hook__subcmd__help,autostart)
                cmd="sway__things__subcmd__hook__subcmd__help__subcmd__autostart"
                ;;
            sway__things__subcmd__hook__subcmd__help,clipboard)
                cmd="sway__things__subcmd__hook__subcmd__help__subcmd__clipboard"
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
            sway__things__subcmd__hook__subcmd__help,launch)
                cmd="sway__things__subcmd__hook__subcmd__help__subcmd__launch"
                ;;
            sway__things__subcmd__hook__subcmd__help,mirror)
                cmd="sway__things__subcmd__hook__subcmd__help__subcmd__mirror"
                ;;
            sway__things__subcmd__hook__subcmd__help,new-display)
                cmd="sway__things__subcmd__hook__subcmd__help__subcmd__new__subcmd__display"
                ;;
            sway__things__subcmd__hook__subcmd__help,wallpaper)
                cmd="sway__things__subcmd__hook__subcmd__help__subcmd__wallpaper"
                ;;
            sway__things__subcmd__hook__subcmd__help,workspaces)
                cmd="sway__things__subcmd__hook__subcmd__help__subcmd__workspaces"
                ;;
            sway__things__subcmd__wallpaper,help)
                cmd="sway__things__subcmd__wallpaper__subcmd__help"
                ;;
            sway__things__subcmd__wallpaper,next)
                cmd="sway__things__subcmd__wallpaper__subcmd__next"
                ;;
            sway__things__subcmd__wallpaper__subcmd__help,help)
                cmd="sway__things__subcmd__wallpaper__subcmd__help__subcmd__help"
                ;;
            sway__things__subcmd__wallpaper__subcmd__help,next)
                cmd="sway__things__subcmd__wallpaper__subcmd__help__subcmd__next"
                ;;
            *)
                ;;
        esac
    done

    case "${cmd}" in
        sway__things)
            opts="-h -V --color --help --version get set reset show-generated doctor open hook screenshot record clipboard wallpaper help"
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
        sway__subcmd__things__subcmd__clipboard)
            opts="-h --color --help pick clear help"
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
        sway__subcmd__things__subcmd__clipboard__subcmd__clear)
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
        sway__subcmd__things__subcmd__clipboard__subcmd__help)
            opts="pick clear help"
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
        sway__subcmd__things__subcmd__clipboard__subcmd__help__subcmd__clear)
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
        sway__subcmd__things__subcmd__clipboard__subcmd__help__subcmd__help)
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
        sway__subcmd__things__subcmd__clipboard__subcmd__help__subcmd__pick)
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
        sway__subcmd__things__subcmd__clipboard__subcmd__pick)
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
            opts="-h --device --collection --json --color --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --device)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --collection)
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
            opts="get set reset show-generated doctor open hook screenshot record clipboard wallpaper help"
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
        sway__subcmd__things__subcmd__help__subcmd__clipboard)
            opts="pick clear"
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
        sway__subcmd__things__subcmd__help__subcmd__clipboard__subcmd__clear)
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
        sway__subcmd__things__subcmd__help__subcmd__clipboard__subcmd__pick)
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
            opts="gsettings displays workspaces mirror new-display wallpaper clipboard launch autostart"
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
        sway__subcmd__things__subcmd__help__subcmd__hook__subcmd__autostart)
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
        sway__subcmd__things__subcmd__help__subcmd__hook__subcmd__clipboard)
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
        sway__subcmd__things__subcmd__help__subcmd__hook__subcmd__launch)
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
        sway__subcmd__things__subcmd__help__subcmd__hook__subcmd__new__subcmd__display)
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
        sway__subcmd__things__subcmd__help__subcmd__hook__subcmd__wallpaper)
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
        sway__subcmd__things__subcmd__help__subcmd__record)
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
        sway__subcmd__things__subcmd__help__subcmd__screenshot)
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
        sway__subcmd__things__subcmd__help__subcmd__wallpaper)
            opts="next"
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
        sway__subcmd__things__subcmd__help__subcmd__wallpaper__subcmd__next)
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
        sway__subcmd__things__subcmd__hook)
            opts="-h --color --help gsettings displays workspaces mirror new-display wallpaper clipboard launch autostart help"
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
        sway__subcmd__things__subcmd__hook__subcmd__autostart)
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
        sway__subcmd__things__subcmd__hook__subcmd__clipboard)
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
            opts="gsettings displays workspaces mirror new-display wallpaper clipboard launch autostart help"
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
        sway__subcmd__things__subcmd__hook__subcmd__help__subcmd__autostart)
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
        sway__subcmd__things__subcmd__hook__subcmd__help__subcmd__clipboard)
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
        sway__subcmd__things__subcmd__hook__subcmd__help__subcmd__launch)
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
        sway__subcmd__things__subcmd__hook__subcmd__help__subcmd__new__subcmd__display)
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
        sway__subcmd__things__subcmd__hook__subcmd__help__subcmd__wallpaper)
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
        sway__subcmd__things__subcmd__hook__subcmd__launch)
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
        sway__subcmd__things__subcmd__hook__subcmd__new__subcmd__display)
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
        sway__subcmd__things__subcmd__hook__subcmd__wallpaper)
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
        sway__subcmd__things__subcmd__record)
            opts="-h --color --help region display"
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
            opts="-h --device --collection --yes --overwrite --color --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --device)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --collection)
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
        sway__subcmd__things__subcmd__screenshot)
            opts="-h --color --help region window display all"
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
        sway__subcmd__things__subcmd__set)
            opts="-h --device --collection --host --shared --yes --overwrite --color --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 2 ]] ; then
                COMPREPLY=( $(compgen -W "${opts}" -- "${cur}") )
                return 0
            fi
            case "${prev}" in
                --device)
                    COMPREPLY=($(compgen -f "${cur}"))
                    return 0
                    ;;
                --collection)
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
            opts="-h --color --help 10-input.conf 20-output.conf 30-appearance.conf 40-behavior.conf 50-bindings.conf 70-autostart.conf 80-hooks.conf 90-effects.conf"
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
        sway__subcmd__things__subcmd__wallpaper)
            opts="-h --color --help next help"
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
        sway__subcmd__things__subcmd__wallpaper__subcmd__help)
            opts="next help"
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
        sway__subcmd__things__subcmd__wallpaper__subcmd__help__subcmd__help)
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
        sway__subcmd__things__subcmd__wallpaper__subcmd__help__subcmd__next)
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
        sway__subcmd__things__subcmd__wallpaper__subcmd__next)
            opts="-h --device --due --color --help"
            if [[ ${cur} == -* || ${COMP_CWORD} -eq 3 ]] ; then
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
    esac
}

if [[ "${BASH_VERSINFO[0]}" -eq 4 && "${BASH_VERSINFO[1]}" -ge 4 || "${BASH_VERSINFO[0]}" -gt 4 ]]; then
    complete -F _sway__things -o nosort -o bashdefault -o default sway-things
else
    complete -F _sway__things -o bashdefault -o default sway-things
fi
