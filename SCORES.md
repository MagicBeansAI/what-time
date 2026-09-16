# Model scores by build

One row per shipped (or candidate) checkpoint. Append a row whenever the
weights change; never edit old rows. All numbers are exact token-label +
boundary sequences on held-out data (the training run's epoch report),
plus the fail-closed corpus gates from `cargo test`. In-distribution
caveat: the structural and extra-family slices share generator
structure with training; the LLM slices do not.

| Build | Date | Checkpoint | Structural heldout | Extra families | LLM heldout | Pinned corpora (cargo test) | Notes |
|---|---|---|---|---|---|---|---|
| (pre-V2 baseline) | 2026-09-16 | original int8 export | 94.14% | 68.97% | — | 526 gold · 15 multilingual · 18 occurrences | before the V2 corpus batch |
| v0.1.0 | 2026-09-16 | `corpus-v2-complete` | 95.34% | 99.28% | 99.80% (V2 targeted, 1k) | 526 · 15 · 18 · parity | published to npm; tareekh/roz/Devanagari-numeral batch |
| v0.2.1 | 2026-09-16 | `quarter-gaps-r2` | 96.30% | 99.42% | 99.55% (union heldout, 2k) | 568 pinned (0 known gaps) · 21 · 21 · parity | quarter periods + Diwali table; both 0.2.0 gaps closed |
| v0.2.0 | 2026-09-16 | `llm-ext-r5` | 96.06% | 99.50% | 99.45% (union heldout, 2k incl. 1k external) | 568 pinned (2 known gaps: `user-ordinal-hindi`, `user-sava-utth`) · 15 · 18 · parity | external LLM batch + tarikh/postposed-range/inflected-ordinal compiler fixes; corpus pool 40,250 accepted |

Known open gaps for the next round: `next quarter` needs `Unit::quarter`
+ resolve arithmetic (245 gated rows waiting); दिवाली needs a
holiday-date table (62 rows); the two KNOWN_GAPS corpus cases close via
training, not editing.
