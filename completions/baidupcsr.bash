# bash completion for baidupcsr-cli

_baidupcsr_cli() {
    local cur prev words cword
    if declare -F _init_completion >/dev/null; then
        _init_completion || return
    else
        words=("${COMP_WORDS[@]}")
        cword=$COMP_CWORD
        cur="${COMP_WORDS[COMP_CWORD]}"
        prev="${COMP_WORDS[COMP_CWORD - 1]}"
    fi

    case "$prev" in
        --config-dir)
            _filedir -d
            return
            ;;
    esac

    case "$cword" in
        1)
            COMPREPLY=($(compgen -W "tui doctor" -- "$cur"))
            return
            ;;
    esac

    case "${words[1]}" in
        doctor)
            COMPREPLY=($(compgen -W "--redact --config-dir" -- "$cur"))
            return
            ;;
        tui)
            COMPREPLY=()
            return
            ;;
    esac
}

complete -F _baidupcsr_cli baidupcsr-cli
