# Reports prompts, commands and their exit codes (OSC 133) and the directory (OSC 7) to
# remacs's terminal, as fish does by itself.
[[ -n $__remacs_hooked ]] && builtin return
builtin typeset -g __remacs_hooked=1 __remacs_ran=

__remacs_precmd() {
    builtin local code=$?
    [[ -n $__remacs_ran ]] && builtin printf '\e]133;D;%s\a' $code
    __remacs_ran=
    builtin printf '\e]7;file://%s%s\a' $HOST $PWD
    builtin printf '\e]133;A\a'
}

__remacs_preexec() {
    __remacs_ran=1
    builtin printf '\e]133;C\a'
}

builtin autoload -Uz add-zsh-hook
add-zsh-hook precmd __remacs_precmd
add-zsh-hook preexec __remacs_preexec
