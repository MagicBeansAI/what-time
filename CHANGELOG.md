# Changelog

Short and honest. Scores per build live in [`SCORES.md`](SCORES.md).

## 0.2.0

- Hindi/Hinglish postposed date and clock ranges: `15 tareekh se 20 tareekh tak`,
  `sava char se paune paanch tak` (both markers follow what they govern).
- `tarikh` romanization, inflected Hindi ordinals (दूसरे/तीसरे/चौथे/पाँचवे) in the compiler.
- Retrained on a 20k-line external-model corpus plus an authored supplement:
  generator held-out 95.3% → 96.1%, LLM held-out 99.45% on a harder 2k union slice.
- Playground: version badge (read from the wasm itself), new trilingual examples,
  and a fix for the initial parse silently no-op'ing on page load.
- Regression corpus wired into the release gate and grown 526 → 568 pinned cases.
- Known gaps (diagnostics, never wrong dates): `next quarter`, दिवाली and other
  movable holiday names, ordinal-anchored recurrences via the live model
  (`हर महीने के दूसरे सोमवार को` compiles but the tagger drops the ordinal).

## 0.1.0

- First public release: trilingual (English/हिन्दी/Hinglish) neural schedule
  parser — 144k-parameter int8 transformer, deterministic compiler, exact
  IANA/DST resolver, RFC 5545 export; wasm + npm, CLI, offline playground.
