"""Feature-table constants shared by the trainer and the model.

They mirror `rust/what-time/src/tokenizer.rs` (`ROWS_PER_TOKEN`,
`FEATURE_ROWS`): the Rust featurizer writes row blocks of this width, and
the embedding table has this many rows, the last id padding unused slots.
"""

ROWS_PER_TOKEN = 20
FEATURE_ROWS = 2116
PADDING_ROW = FEATURE_ROWS
# Word-form rows (whole-word high bits, prefix, suffix) after the shape rows.
LEXICAL_ROWS = range(580, FEATURE_ROWS)
