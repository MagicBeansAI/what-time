# Third-party notices

`what-time` is developed and evaluated with third-party software. Their
names identify upstream projects and do not imply endorsement.

The published packages have **no runtime dependencies** beyond the WebAssembly
module itself. Everything listed here is a development, training, or
benchmark dependency and is not bundled into the parser.

- The WebAssembly bindings use [wasm-bindgen](https://github.com/rustwasm/wasm-bindgen)
  (MIT/Apache-2.0), generated with `wasm-bindgen-cli`.
- Timezone resolution uses [jiff](https://github.com/BurntSushi/jiff)
  (MIT/Apache-2.0) with its bundled IANA Time Zone Database.
- Serialization uses [serde](https://github.com/serde-rs/serde) and
  [serde_json](https://github.com/serde-rs/json) (MIT/Apache-2.0).
- Training uses [PyTorch](https://github.com/pytorch/pytorch) and
  [NumPy](https://github.com/numpy/numpy), pinned in
  `packages/training/pyproject.toml` and `uv.lock`. Label supervision is
  produced by this repository's own renderers and by gate-checked
  LLM-generated corpora; no third-party corpus assigns labels.
- Background prose in generated training data draws from filtered
  [Tatoeba](https://tatoeba.org) sentences.
