#!/usr/bin/env sh
# Builds `model-wasm` (Model in the browser) with wasm-pack into crates/model-wasm/pkg, or into $1.
#
# Needs: the wasm32-unknown-unknown target, wasm-pack, and a clang that can target wasm32
# (Apple's cannot: `brew install llvm`; set CC_wasm32_unknown_unknown to it if it is not first on PATH).
# tree-sitter's grammars need a libc header set that wasm32-unknown-unknown lacks: the one shipped by
# the `tree-sitter-language` crate is used.
set -eu
cd "$(dirname "$0")/.."
cargo fetch --target wasm32-unknown-unknown >/dev/null
headers=$(ls -d "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/tree-sitter-language-*/wasm/include | tail -1)
: "${CC_wasm32_unknown_unknown:=$(command -v clang)}"
[ -x /opt/homebrew/opt/llvm/bin/clang ] && CC_wasm32_unknown_unknown=/opt/homebrew/opt/llvm/bin/clang
export CC_wasm32_unknown_unknown
[ -x /opt/homebrew/opt/llvm/bin/llvm-ar ] && export AR_wasm32_unknown_unknown=/opt/homebrew/opt/llvm/bin/llvm-ar
export CFLAGS_wasm32_unknown_unknown="-I$headers ${CFLAGS_wasm32_unknown_unknown:-}"
out=${1:-pkg}
wasm-pack build crates/model-wasm --release --target web --out-dir "$out" --out-name model_wasm
