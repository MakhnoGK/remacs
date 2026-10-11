# remacs started zsh with ZDOTDIR here: put yours back, read your .zshenv, then hook
# the prompt of an interactive shell. zsh reads the rest of your files from your ZDOTDIR.
if [[ -n ${REMACS_ZSH_ZDOTDIR+set} ]]; then
    builtin export ZDOTDIR=$REMACS_ZSH_ZDOTDIR
    builtin unset REMACS_ZSH_ZDOTDIR
else
    builtin unset ZDOTDIR
fi

{
    builtin typeset __remacs_file=${ZDOTDIR-$HOME}/.zshenv
    [[ ! -r $__remacs_file ]] || builtin source -- $__remacs_file
} always {
    if [[ -o interactive ]]; then
        builtin source -- ${${(%):-%x}:A:h}/remacs.zsh
    fi
    builtin unset __remacs_file
}
