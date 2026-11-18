#!/usr/bin/env bash
#
# Runs a sequential probability ratio test (SPRT) between two builds of the
# engine with fastchess. The builds play each other until the results show,
# with 5% error rates, either that the new build is stronger by at least
# elo1 (H1 accepted: the test passes) or that it is stronger by no more than
# elo0 (H0 accepted: the test fails).
#
# usage: tools/sprt.sh <name> <base-engine> <new-engine> [<elo0> <elo1>]
#
#   name          label for the output files games/sprt-<name>.pgn and .log
#   base-engine   the build to compare against
#   new-engine    the build under test
#   elo0, elo1    SPRT bounds, 0 and 10 by default
#
# The reported Elo is from the new build's point of view. Think Time and
# Variety are turned off, so that both builds use their clocks in full and
# always play their best move. Settings can be changed through environment
# variables:
#
#   TC            time control (default 8+0.08)
#   CONCURRENCY   games played at the same time (default 4)
#   BOOK          opening book (default tools/openings/random-8ply.epd)
#   ROUNDS        most game pairs to play (default 20000)
#   FASTCHESS     fastchess executable (default tools/bin/fastchess.exe)
#
# Test copies of the builds, not target/release/fishrod.exe itself, so that a
# later cargo build cannot replace an engine that is still playing:
#
#   cp target/release/fishrod.exe tools/bin/fishrod-base.exe

set -euo pipefail

if [ $# -ne 3 ] && [ $# -ne 5 ]; then
    echo "usage: tools/sprt.sh <name> <base-engine> <new-engine> [<elo0> <elo1>]" >&2
    exit 2
fi

name=$1
base=$2
new=$3
elo0=${4:-0}
elo1=${5:-10}

tc=${TC:-8+0.08}
concurrency=${CONCURRENCY:-4}
book=${BOOK:-tools/openings/random-8ply.epd}
rounds=${ROUNDS:-20000}
fastchess=${FASTCHESS:-tools/bin/fastchess.exe}

for file in "$base" "$new" "$book" "$fastchess"; do
    if [ ! -f "$file" ]; then
        echo "error: $file not found" >&2
        exit 1
    fi
done

# fastchess cannot start an engine from a bare relative path on Windows.
engine_path() {
    case "$1" in
        /* | ./* | ?:*) echo "$1" ;;
        *) echo "./$1" ;;
    esac
}

mkdir -p games
pgn="games/sprt-$name.pgn"
log="games/sprt-$name.log"
rm -f "$pgn"

"$fastchess" \
    -engine cmd="$(engine_path "$new")" name=new \
    -engine cmd="$(engine_path "$base")" name=base \
    -each tc="$tc" option.Hash=16 option.Threads=1 "option.Think Time=0" option.Variety=0 \
    -openings file="$book" format=epd order=random \
    -rounds "$rounds" -games 2 -repeat -concurrency "$concurrency" \
    -sprt elo0="$elo0" elo1="$elo1" alpha=0.05 beta=0.05 \
    -resign movecount=3 score=600 twosided=true \
    -draw movenumber=40 movecount=8 score=10 \
    -ratinginterval 20 \
    -config outname="games/sprt-$name.config.json" \
    -pgnout file="$pgn" |
    tee "$log"
