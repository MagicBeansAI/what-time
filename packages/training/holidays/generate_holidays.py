"""Regenerate rust/what-time/assets/holidays.json from the curated
reference plus rule-based entries.

One command rebuilds the whole asset deterministically; CI re-runs it in
--check mode so the committed asset can never drift from its sources.
Rule-based holidays (fixed dates, nth-weekday, Easter offsets) are defined
here in code; lunar-calendar festivals come from reference.json, which is
curated from published panchang lists (see its "canonical" note). When an
astronomy-based proposer is added later, it must agree with the reference
or the build fails — the reference is the ratified truth, never the
proposal.
"""

import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent
REFERENCE = ROOT / "reference.json"
ASSET = ROOT.parent.parent.parent / "rust/what-time/assets/holidays.json"

# rule-based entries: key, region, kind, fields
RULED = [
    {"key": "christmas", "region": "global", "kind": "fixed", "month": 12, "day": 25},
    {"key": "christmas-eve", "region": "global", "kind": "fixed", "month": 12, "day": 24},
    {"key": "new-year", "region": "global", "kind": "fixed", "month": 1, "day": 1},
    {"key": "new-years-eve", "region": "global", "kind": "fixed", "month": 12, "day": 31},
    {"key": "halloween", "region": "global", "kind": "fixed", "month": 10, "day": 31},
    {"key": "valentines", "region": "global", "kind": "fixed", "month": 2, "day": 14},

    {"key": "thanksgiving", "region": "US", "kind": "nthWeekday", "month": 11, "ordinal": 4, "weekday": "TH"},
    {"key": "independence-day", "region": "US", "kind": "fixed", "month": 7, "day": 4},
    {"key": "juneteenth", "region": "US", "kind": "fixed", "month": 6, "day": 19},
    {"key": "memorial-day", "region": "US", "kind": "nthWeekday", "month": 5, "ordinal": -1, "weekday": "MO"},
    {"key": "labor-day", "region": "US", "kind": "nthWeekday", "month": 9, "ordinal": 1, "weekday": "MO"},
    {"key": "mlk-day", "region": "US", "kind": "nthWeekday", "month": 1, "ordinal": 3, "weekday": "MO"},
    {"key": "presidents-day", "region": "US", "kind": "nthWeekday", "month": 2, "ordinal": 3, "weekday": "MO"},
    {"key": "mothers-day", "region": "US", "kind": "nthWeekday", "month": 5, "ordinal": 2, "weekday": "SU"},
    {"key": "fathers-day", "region": "US", "kind": "nthWeekday", "month": 6, "ordinal": 3, "weekday": "SU"},
    {"key": "columbus-day", "region": "US", "kind": "nthWeekday", "month": 10, "ordinal": 2, "weekday": "MO"},
    {"key": "veterans-day", "region": "US", "kind": "fixed", "month": 11, "day": 11},

    {"key": "boxing-day", "region": "UK", "kind": "fixed", "month": 12, "day": 26},
    {"key": "good-friday", "region": "UK", "kind": "easterOffset", "offset": -2},
    {"key": "easter", "region": "UK", "kind": "easterOffset", "offset": 0},
    {"key": "easter-monday", "region": "UK", "kind": "easterOffset", "offset": 1},
    {"key": "mothering-sunday", "region": "UK", "kind": "easterOffset", "offset": -21},
    {"key": "early-may-bank", "region": "UK", "kind": "nthWeekday", "month": 5, "ordinal": 1, "weekday": "MO"},
    {"key": "spring-bank", "region": "UK", "kind": "nthWeekday", "month": 5, "ordinal": -1, "weekday": "MO"},
    {"key": "summer-bank", "region": "UK", "kind": "nthWeekday", "month": 8, "ordinal": -1, "weekday": "MO"},
    {"key": "st-patricks", "region": "IE", "kind": "fixed", "month": 3, "day": 17},
    {"key": "guy-fawkes", "region": "UK", "kind": "fixed", "month": 11, "day": 5},

    {"key": "gandhi-jayanti", "region": "IN", "kind": "fixed", "month": 10, "day": 2},
    {"key": "canada-day", "region": "CA", "kind": "fixed", "month": 7, "day": 1},
    {"key": "canadian-thanksgiving", "region": "CA", "kind": "nthWeekday", "month": 10, "ordinal": 2, "weekday": "MO"},
    {"key": "australia-day", "region": "AU", "kind": "fixed", "month": 1, "day": 26},
    {"key": "anzac-day", "region": "AU", "kind": "fixed", "month": 4, "day": 25},
]

# sanity windows for tabulated festivals: (earliest_month_day, latest_month_day)
WINDOWS = {
    "diwali": ((10, 1), (11, 30)),
    "holi": ((2, 20), (3, 31)),
    "mahashivratri": ((2, 1), (3, 31)),
    "raksha-bandhan": ((7, 15), (8, 31)),
    "ganesh-chaturthi": ((8, 15), (9, 30)),
    "dussehra": ((9, 1), (10, 31)),
    "karwa-chauth": ((10, 1), (11, 15)),
    "eid-ul-fitr": ((2, 1), (4, 30)),
    "eid-ul-adha": ((4, 15), (6, 30)),
}


def build_entries() -> list[dict]:
    reference = json.loads(REFERENCE.read_text(encoding="utf-8"))
    entries = [dict(rule) for rule in RULED]
    for key, spec in reference["tabulated"].items():
        dates = {k: v for k, v in spec.items() if k != "predicted"}
        if not dates:
            continue
        for year, (month, day) in dates.items():
            low, high = WINDOWS[key]
            if not (low <= (month, day) <= high):
                raise SystemExit(
                    f"{key} {year}: {month}/{day} outside sanity window {low}–{high}; "
                    "check the reference row before generating"
                )
        entry = {"key": key, "region": "IN", "kind": "tabulated", "dates": dates}
        if spec.get("predicted"):
            entry["predicted"] = True
        entries.append(entry)
    keys = [entry["key"] for entry in entries]
    if len(keys) != len(set(keys)):
        raise SystemExit("duplicate holiday keys")
    return entries


def main() -> None:
    entries = build_entries()
    rendered = json.dumps({"entries": entries}, indent=2, ensure_ascii=False) + "\n"
    if "--check" in sys.argv:
        current = ASSET.read_text(encoding="utf-8")
        if current != rendered:
            sys.exit(
                "assets/holidays.json is stale; re-run "
                "packages/training/holidays/generate_holidays.py and commit"
            )
        print(f"asset in sync ({len(entries)} entries)")
        return
    ASSET.write_text(rendered, encoding="utf-8")
    print(f"wrote {len(entries)} entries to {ASSET}")


if __name__ == "__main__":
    main()
