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

Files: `index.html` (page) and `wasm/` (bindgen glue + module). The wasm
embeds the int8-quantized weights (~144k parameters) and the bundled IANA
timezone database, so timezone resolution is exact and offline.
