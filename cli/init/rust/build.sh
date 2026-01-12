#!/bin/bash
set -e
mkdir -p wit
asterai pkg -o wit/package.wasm -w wit/package.wit
cargo component build --release
