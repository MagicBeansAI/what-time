"""Corpus safety checks; run with uv run python torch/test-corpus-v2.py."""

from collections import defaultdict
from pathlib import Path
import runpy
import subprocess
import unittest

from natural import RESERVED, RESERVED_DURATION
from validate_llm_corpus import LABELS, oracle_failures, span_offsets, split_tokens

HERE = Path(__file__).resolve().parent
GENERATOR = runpy.run_path(str(HERE / "generate-llm-corpus-v2.py"))
PREPARE = runpy.run_path(str(HERE / "prepare-llm-corpus-v2.py"))


class CorpusV2Tests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.rows, cls.metadata, cls.counts = GENERATOR["generate"](20260916)

    def test_corpus_contract(self):
        self.assertEqual(len(self.rows), 20000)
        self.assertEqual(len({r["text"] for r in self.rows}), 20000)
        for row in self.rows:
            words, labels = zip(*row["tokens"])
            self.assertEqual(row["text"], " ".join(words))
            self.assertEqual(list(words), split_tokens(row["text"]))
            self.assertLess(len(words), 40)
            self.assertLessEqual(set(labels), LABELS)
            self.assertFalse(any(p in row["text"].lower() for p in RESERVED + RESERVED_DURATION))

    def test_contrasts_and_negative_labels(self):
        groups = defaultdict(list)
        for row, meta in zip(self.rows, self.metadata):
            groups[(meta["family"], meta["group"])].append(row)
            if meta["family"] == "negative":
                self.assertTrue(all(label == "O" for _, label in row["tokens"]))
        for (family, _), rows in groups.items():
            self.assertEqual(len(rows), {"daypart": 4, "tense": 2}.get(family, 1))
            if family == "tense":
                semantic = lambda r: [(w, label) for w, label in r["tokens"] if label != "O"]
                self.assertEqual(semantic(rows[0]), semantic(rows[1]))

    def test_evaluation_split_does_not_separate_contrasts(self):
        rows = [{"id": str(i), "text": row["text"]} for i, row in enumerate(self.rows)]
        metadata = {m["text"]: m for m in self.metadata}
        heldout = PREPARE["split_groups"](rows, metadata)
        self.assertEqual(len(heldout), 1000)
        held_ids = {row["id"] for row in heldout}
        held_groups = {metadata[row["text"]]["group"] for row in heldout}
        training_groups = {metadata[row["text"]]["group"] for row in rows if row["id"] not in held_ids}
        self.assertFalse(held_groups & training_groups)

    def test_tokenizer_exceptions_and_utf16_offsets(self):
        self.assertEqual(split_tokens("2MRW, १५ तारीख at ८:३० pm"),
                         ["2MRW", ",", "१५", "तारीख", "at", "८", ":", "३०", "pm"])
        self.assertEqual(split_tokens("24th 2mrwx"), ["24", "th", "2", "mrwx"])
        self.assertEqual(span_offsets("🎉 कल ८"), [(0, 2, "🎉"), (3, 5, "कल"), (6, 7, "८")])

    def test_oracle_must_account_for_every_row(self):
        completed = lambda code, out, err: subprocess.CompletedProcess([], code, out, err)
        valid = completed(0, "validated 2 lines, 0 failures (0.0%)\n", "")
        self.assertEqual(oracle_failures(valid, 2), {})
        mismatch = completed(1, "validated 1 lines, 1 failures (100.0%)\n",
                             'line 2: 2 labels for 3 non-space tokens: "bad"\n')
        self.assertEqual(set(oracle_failures(mismatch, 2)), {1})
        for result in [completed(-9, "", ""), completed(1, "", "panic"),
                       completed(0, "validated 1 lines, 0 failures (0.0%)", ""),
                       completed(1, "validated 2 lines, 1 failures (50.0%)", "")]:
            with self.assertRaises(RuntimeError):
                oracle_failures(result, 2)


if __name__ == "__main__":
    unittest.main()
