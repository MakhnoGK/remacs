# remacs started bash with --rcfile here, never as a login shell, which reads no rcfile:
# read the startup files bash would have read, then report prompts, commands and their
# exit codes (OSC 133) and the directory (OSC 7) to remacs's terminal, as fish does by
# itself.
if [ -n "$REMACS_BASH_INJECT" ]; then
    builtin unset REMACS_BASH_INJECT
    if [ -n "$REMACS_BASH_LOGIN" ]; then
        if [ -z "$REMACS_BASH_NOPROFILE" ]; then
            [ -r /etc/profile ] && builtin source /etc/profile
            for __remacs_file in "$HOME/.bash_profile" "$HOME/.bash_login" "$HOME/.profile"; do
                if [ -r "$__remacs_file" ]; then
                    builtin source "$__remacs_file"
                    break
                fi
            done
        fi
    elif [ -z "$REMACS_BASH_NORC" ]; then
        __remacs_file=${REMACS_BASH_RCFILE:-$HOME/.bashrc}
        [ -r "$__remacs_file" ] && builtin source "$__remacs_file"
    fi
    builtin unset __remacs_file REMACS_BASH_LOGIN REMACS_BASH_NOPROFILE REMACS_BASH_NORC \
        REMACS_BASH_RCFILE
fi

if [[ $- == *i* && -z $__remacs_hooked ]]; then
    __remacs_hooked=1
    __remacs_ran=
    __remacs_at_prompt=

    # First in PROMPT_COMMAND, where $? is still the command's.
    __remacs_status() {
        local code=$?
        __remacs_at_prompt=
        [[ -n $__remacs_ran ]] && builtin printf '\033]133;D;%s\007' "$code"
        __remacs_ran=
        return $code
    }

    # Last in PROMPT_COMMAND, right before the prompt.
    __remacs_prompt() {
        local code=$?
        builtin printf '\033]7;file://%s%s\007' "$HOSTNAME" "$PWD"
        builtin printf '\033]133;A\007'
        __remacs_at_prompt=1
        return $code
    }

    # The DEBUG trap runs before every command; the first after the prompt is the line
    # typed, unless it was empty and PROMPT_COMMAND comes straight back.
    __remacs_preexec() {
        [[ -n $__remacs_at_prompt && $BASH_COMMAND != __remacs_status ]] || return
        __remacs_at_prompt=
        __remacs_ran=1
        builtin printf '\033]133;C\007'
    }

    [[ -z $(builtin trap -p DEBUG) ]] && builtin trap '__remacs_preexec' DEBUG
    PROMPT_COMMAND=$'__remacs_status\n'"${PROMPT_COMMAND}"$'\n__remacs_prompt'
fi
