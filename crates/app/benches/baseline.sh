#!/bin/sh
# The performance baseline (docs/performance/baseline.md): for each focused language, opens
# a pinned project in the release build with default settings, moves the cursor by keys,
# then measures idle CPU and memory with its language servers running.
#
# Needs Accessibility for the terminal (System Events sends the keys), the servers on PATH
# (rust-analyzer, clangd, gopls, typescript-language-server) and nobody at the machine.
# Usage: crates/app/benches/baseline.sh [rust|c|go|ts ...]
set -eu

repo=$(cd "$(dirname "$0")/../../.." && pwd)
work=${BASELINE_DIR:-$HOME/.cache/remacs-baseline}
settle=${SETTLE:-90} # seconds the servers get to index before anything is measured
idle=${IDLE:-60}     # seconds idle CPU is taken over
keys=${KEYS:-150}    # down, then as many up, presses
PATH=$HOME/go/bin:$PATH

# language repository tag file
projects='rust https://github.com/BurntSushi/ripgrep 14.1.1 crates/core/flags/defs.rs
c https://github.com/redis/redis 7.2.4 src/server.c
go https://github.com/spf13/cobra v1.8.1 command.go
ts https://github.com/pmndrs/zustand v4.5.5 src/middleware/devtools.ts'

descendants() {
    for child in $(pgrep -P "$1"); do
        echo "$child"
        descendants "$child"
    done
}

# Seconds of CPU the processes have used.
cpu_seconds() {
    ps -o time= -p "$(echo "$@" | tr ' ' ',')" |
        awk -F: '{ s += NF == 3 ? $1 * 3600 + $2 * 60 + $3 : $1 * 60 + $2 } END { print s + 0 }'
}

# Megabytes of physical footprint the processes have together.
footprint_mb() {
    for pid in "$@"; do footprint "$pid" 2>/dev/null | grep -m1 'Footprint:'; done |
        awk '{ for (i = 1; i < NF; i++) if ($i == "Footprint:") {
            n = $(i + 1); u = $(i + 2)
            s += u == "GB" ? n * 1024 : u == "MB" ? n : u == "KB" ? n / 1024 : n / 1048576 } }
            END { printf "%.0f", s }'
}

names() {
    for pid in "$@"; do basename "$(ps -o comm= -p "$pid")"; done | sort | uniq -c |
        awk '{ printf "%s%s", (NR > 1 ? ", " : ""), ($1 > 1 ? $2 " ×" $1 : $2) }'
}

measure() {
    language=$1 url=$2 tag=$3 file=$4
    dir=$work/projects/$language
    [ -d "$dir" ] || git clone -q -c advice.detachedHead=false --depth 1 --branch "$tag" "$url" "$dir"
    home=$work/home-$language
    rm -rf "$home"
    mkdir -p "$home/remacs/config"
    echo 'frame-latency = true' >"$home/remacs/config/logs.toml"
    echo 'from-login-shell = "never"' >"$home/remacs/config/environment.toml"
    XDG_CONFIG_HOME=$home "$repo/target/release/remacs" "$dir/$file" </dev/null >/dev/null 2>&1 &
    pid=$!
    sleep "$settle"

    osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $pid) to true"
    sleep 2
    osascript -e 'tell application "System Events"' \
        -e "repeat $keys times" -e 'key code 125' -e 'delay 0.05' -e 'end repeat' \
        -e "repeat $keys times" -e 'key code 126' -e 'delay 0.05' -e 'end repeat' \
        -e 'end tell'

    sleep 5
    servers=$(descendants "$pid")
    app_before=$(cpu_seconds "$pid")
    servers_before=$(cpu_seconds $servers)
    sleep "$idle"
    app_cpu=$(echo "$(cpu_seconds "$pid") $app_before $idle" | awk '{ printf "%.2f", ($1 - $2) / $3 * 100 }')
    servers_cpu=$(echo "$(cpu_seconds $servers) $servers_before $idle" | awk '{ printf "%.2f", ($1 - $2) / $3 * 100 }')
    app_mb=$(footprint_mb "$pid")
    servers_mb=$(footprint_mb $servers)
    server_names=$(names $servers)

    latency=$(grep '^frame-latency' "$home/remacs/state/remacs.log" | awk '{ print $2 / 1000 }' | sort -n |
        awk '{ v[NR] = $1 } END { printf "%d | %.1f | %.1f | %.1f", NR, v[int(NR / 2) + 1], v[int(NR * 0.95) + 1], v[NR] }')

    kill "$pid" $servers 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    echo "| $language | ${url#https://github.com/} $tag | $latency | $app_cpu | $servers_cpu | $app_mb | $servers_mb | $server_names |"
}

(cd "$repo" && cargo build --release -q -p remacs-app)
echo '| Language | Project | Frames | Median ms | p95 ms | Max ms | Idle CPU app % | Idle CPU servers % | App MB | Servers MB | Servers |'
echo '|---|---|---|---|---|---|---|---|---|---|---|'
echo "$projects" | while read -r language url tag file; do
    if [ $# -eq 0 ] || echo " $* " | grep -q " $language "; then
        measure "$language" "$url" "$tag" "$file"
    fi
done
