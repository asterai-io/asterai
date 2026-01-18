#!/bin/bash
set -e
bash ./build.sh
asterai component push --pkg wit/package.wasm --plugin target/wasm32-wasip1/release/component.wasm
