#!/bin/bash
set -e
if [ -z "$1" ]; then
  echo "Deploying without agent ID. Optionally, pass it: $0 <agent ID>"
fi
bash ./build.sh
asterai deploy --pkg wit/package.wasm --plugin target/wasm32-wasip1/release/plugin.wasm -a "$1"
