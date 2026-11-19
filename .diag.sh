set -uo pipefail
export PATH="$HOME/.cargo/bin:$PWD/.sc041/bin:$PATH"
export TMPDIR=$HOME/builds/convert-lead/tmp; mkdir -p "$TMPDIR"
cargo build --release -j 3 -p dutchman --locked > $TMPDIR/arcbuild.log 2>&1 || { tail -30 $TMPDIR/arcbuild.log; exit 1; }
BIN=$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys;print(json.load(sys.stdin)["target_directory"])')/release/dutchman
mkdir -p $HOME/builds/convert-lead/railcore/bin; cp "$BIN" $HOME/builds/convert-lead/railcore/bin/dutchman-arc
cd $HOME/builds/convert-lead/railcore
nice -n 10 bin/dutchman-arc convert "$HOME/Documents/GitHub/railcore-f3z/RailCore II 300ZL - Nov 2019.f3z" -o out/arc-300zl.sc --json --quiet --yes --log out/arc-300zl.log > out/arc-300zl.json 2> out/arc-300zl.err; echo "exit $?"
