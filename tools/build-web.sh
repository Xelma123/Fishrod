#!/usr/bin/env bash
#
# Builds the web version of the engine: compiles it to WebAssembly and puts
# the module next to the page in web/www. Serve that folder to try it, for
# example with: python -m http.server 8000 --directory web/www
#
# usage: tools/build-web.sh

set -euo pipefail

cargo build --release --target wasm32-wasip1 -p fishrod-web
cp target/wasm32-wasip1/release/fishrod_web.wasm web/www/fishrod.wasm
echo "web/www/fishrod.wasm: $(wc -c < web/www/fishrod.wasm) bytes"
