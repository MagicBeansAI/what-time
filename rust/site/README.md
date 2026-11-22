# Static browser playground

The parser compiled to WebAssembly — the trilingual (English/Hindi/Hinglish)
transformer model runs entirely in the browser; there is no server. Serve
this directory from any static host:

- **`what-time.html`** — one self-contained file (wasm inlined as base64).
  Open it directly from disk; no server, works offline.
- **`index.html` + `wasm/`** — the multi-file variant; serve over http(s)
  (ES module loading needs it, `file://` will not fetch the wasm):

  ```sh
  python3 -m http.server --directory . 8788
  ```

Rebuild after changing the library or model weights:

```sh
./build.sh   # wasm + glue; the page itself is copied from what-time-web assets
```

GitHub Pages does not use the committed `what-time.html`: on every push to
`main`, `.github/workflows/pages.yml` runs `build.sh` from source, checks the
fresh module turns a recurring phrase into the expected cron lines, and only
then deploys. Rebuild and commit the file anyway, for the offline single-file
download and the npm `wasm/` copy.

Files: `index.html` (page) and `wasm/` (bindgen glue + module). The wasm
embeds the int8-quantized weights (~575k parameters) and the bundled IANA
timezone database, so timezone resolution is exact and offline. It is built
with WebAssembly SIMD (`.cargo/config.toml`, `build.sh`): Chrome/Edge 91+,
Firefox 89+, Safari 16.4+.
