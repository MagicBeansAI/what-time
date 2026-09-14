#!/bin/sh
# Rebuild the static browser bundle: wasm module + playground page.
set -e
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --quiet -p what-time-wasm --target wasm32-unknown-unknown --release
wasm-bindgen --target web --out-dir site/wasm target/wasm32-unknown-unknown/release/what_time_wasm.wasm
python3 - <<'PY'
import base64, pathlib, re
glue = pathlib.Path("site/wasm/what_time_wasm.js").read_text()
glue = re.sub(r"^export function", "function", glue, flags=re.M)
glue = re.sub(r"^export const", "const", glue, flags=re.M)
glue = re.sub(r"^export \{[^}]*\};\s*$", "", glue, flags=re.M)
glue = glue.replace("import.meta.url", "'./wasm/what_time_wasm_bg.wasm'")
b64 = base64.b64encode(pathlib.Path("site/wasm/what_time_wasm_bg.wasm").read_bytes()).decode()
page = pathlib.Path("site/index.html").read_text()
header = page[page.find("<script type=\"module\">") : page.find("</script>") + len("</script>")]
inline = f'''<script type="module">
{glue}
const WASM_B64 = "{b64}";
const wasmBytes = Uint8Array.from(atob(WASM_B64), (c) => c.charCodeAt(0));
initSync(new WebAssembly.Module(wasmBytes));
const wasmReady = Promise.resolve();
globalThis.wasmParse = parse;
globalThis.wasmExpressions = parse_expressions;
globalThis.wasmReady = wasmReady;
</script>'''
pathlib.Path("site/what-time.html").write_text(page.replace(header, inline))
print("what-time.html regenerated (self-contained)")
PY
echo "site rebuilt: index.html is maintained here; wasm/ and what-time.html are regenerated."
echo "open what-time.html directly, or serve the directory for the multi-file variant."
