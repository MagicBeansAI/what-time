# Corpus-generation brief for an external model

Hand the prompt below to a model that knows nothing about this project. It
generates English schedule sentences with token-level semantic labels. The
output is validated on our side (conventions, compilability, dedupe,
reserved-phrase filter) before it ever reaches training — anything that fails
the gate is dropped, so the model should optimize for *correctness and
consistency*, not volume.

---

## PROMPT (copy everything below this line)

You are building training data for a small neural sequence tagger that reads
short English texts and labels every token with one semantic role, so a
downstream system can extract dates, times, ranges, and recurrence rules.
You write realistic sentences and label the time-related tokens. Be precise
and consistent; a single mislabeled token corrupts training.

### Tokenization

Write each sentence, then label it as a flat list of tokens in reading
order. Split text into tokens exactly like this:

- Letters (including apostrophes inside words) form one token: `don't`,
  `o'clock`, `Tuesday`.
- Digits form one token: `7`, `24`, `2025`.
- Every punctuation mark is its own token: `:` `,` `-` `.` `/`.
- A suffix attached to digits is its own token: `24th` → `24` + `th`;
  `1st` → `1` + `st`; `7:30pm` → `7` + `:` + `30` + `pm`.
- Separate tokens with single spaces in your output, even if your sentence
  uses different spacing; keep the original casing.

Output JSONL, one object per sentence:

```json
{"text": "book dinner for October 2 at eight pm", "tokens": [["book","O"],["dinner","O"],["for","GLUE"],["October","MONTH"],["2","DOM"],["at","GLUE"],["eight","HOUR"],["pm","MERIDIEM"]]}
```

Every token list, joined with single spaces, must reconstruct `text`.

### The label set (use nothing else)

Background / structure:
- `O` — any word outside a time expression.
- `GLUE` — a filler word inside a time expression that carries no meaning of
  its own: articles and prepositions like `the`, `on`, `at`, `for`, `of`,
  `in` (when not part of a duration), plus digit suffixes (`st`, `nd`, `rd`,
  `th`) and separators (`:`, `,`, `-`, `/`) inside expressions.
- `JOIN` — a conjunction joining two dates or weekdays (`and`, `or`).
- `RANGE_START` — opens a range: `from`, `between`.
- `RANGE_END` — closes a range: `to`, `until`, `till`, `through`, `-`, `–`.
- `BOUND_START` / `BOUND_END` — `starting` / `ending` (recurrence anchors).

Numbers and quantities:
- `NUM` — a bare number or number word that is a quantity: `2`, `three`.
- `ORD` — an ordinal selecting an item: `first`, `2nd`, `last` (but `last` in
  `last year` is DEICTIC).
- `COUNT` — a repetition count for recurrences: `3` in `3 times`.
- `TIMES` — the word `times` (as in `3 times a week`).

Dates:
- `REL_DAY` — a whole relative-day phrase, one label per token: `today`,
  `tomorrow`, `yesterday`, and every token of `the day after tomorrow`,
  `day before yesterday` (with or without the leading `the`).
- `NOW` — `now`, `immediately`, `asap`.
- `DEICTIC` — a period modifier: `this`, `next`, `last`, `coming`,
  `previous`, `upcoming`, `past`.
- `UNIT` — a calendar unit noun: `day(s)`, `week(s)`, `month(s)`, `year(s)`,
  `quarter(s)`, `weekday(s)`, `weekend(s)`, `fortnight(s)`.
- `DIR_BEFORE` / `DIR_AFTER` — a direction word tying a quantity or period to
  an anchor: `before`, `after`, `ago`, `from now`, `later`, `hence`,
  `past` (in `past friday`).
- `WEEKDAY` — a weekday name or abbreviation: `Monday`, `Mon`, `Fri`.
- `DAYGROUP` — `weekday` / `weekdays` / `weekend` / `weekends` used as a
  selector on its own (`every weekend at 9`).
- `MONTH` — a month name or abbreviation: `January`, `Jan`, `Sept`.
- `DOM` — a day-of-month number: `2` in `October 2`, `24` in `24th august`.
- `YEAR` — a 2- or 4-digit year: `2025`, `25` in `fall 2025`.
- `EDGE` — `start`, `beginning`, `end` (of a period): `end of next month`.
- `HOLIDAY` — a holiday name, one label per token: `Christmas`,
  `New Years Eve`, `Thanksgiving`, `Independence Day`.

Times:
- `HOUR` — an hour number or hour word: `8`, `08`, `eight`, `noon` is
  TIME_NAMED.
- `MINUTE` / `SECOND` — minute/second digits after a colon: the `30` in
  `7:30`, the `45` in `7:30:45`.
- `MERIDIEM` — `am`, `pm`, `a.m.`, `p.m.`, `o'clock`.
- `TIME_NAMED` — `noon`, `midnight`, `midday`.
- `DAYPART` — `morning`, `afternoon`, `evening`, `night`.
- `CLOCK_OFFSET` — a half/quarter offset phrase: `half past`, `quarter to`,
  `quarter past` (label `past`/`to` CLOCK_OFFSET too in these phrases).

Recurrence and duration:
- `RECUR` — opens or marks a recurrence: `every`, `each`, `alternating`,
  `biweekly`, `daily`, `weekly`, `monthly`, `yearly`, `annually`.
- `FREQ` — a frequency noun where the interval lives: `week` in `twice a
  week`, `day` in `3 times a day`.
- `DUR` — a duration quantity attached to a unit: `90` in `for 90 minutes`,
  `half` in `for half an hour`.
- `EXCEPT` — `except`, `but not`, `excluding`.

### Hard conventions (these are graded)

1. Digits carry the role; the fused suffix is GLUE: `24th` → `["24","DOM"]`,
   `["th","GLUE"]`.
2. Inside a time expression, function words are GLUE, never O. Outside the
   expression, everything is O, never GLUE.
3. `the day after tomorrow` and `day after tomorrow`: every content token is
   REL_DAY, including `day` and `after`.
4. `next`, `this`, `last` before a period noun are DEICTIC; the period noun
   is UNIT (`last year`, `next quarter`, `this week`). `last` before a
   weekday or month name is also DEICTIC (`last Friday`, `last March`).
5. A weekday before a deictic period anchors to it: `friday next week` →
   WEEKDAY, DEICTIC, UNIT. Same for `24th august last year` → DOM, GLUE,
   MONTH, DEICTIC, UNIT.
6. Ranges: `from Sep 4 through September 8` → RANGE_START, MONTH, DOM,
   RANGE_END, MONTH, DOM.
7. `every`/`each` is RECUR even inside longer recurrences (`every other
   Friday` → RECUR, GLUE, WEEKDAY).
8. If the sentence contains no time expression at all, every token is O.
9. Two separate time expressions in one sentence each get their own labels;
   the words between them are O.

### Worked examples

```json
{"text": "the retreat is the day after tomorrow at 9am", "tokens": [["the","O"],["retreat","O"],["is","O"],["the","REL_DAY"],["day","REL_DAY"],["after","REL_DAY"],["tomorrow","REL_DAY"],["at","GLUE"],["9","HOUR"],["am","MERIDIEM"]]}
{"text": "payday is the last Friday of each month", "tokens": [["payday","O"],["is","O"],["the","GLUE"],["last","ORD"],["Friday","WEEKDAY"],["of","GLUE"],["each","RECUR"],["month","UNIT"]]}
{"text": "out of office from Dec 22 until Jan 2", "tokens": [["out","O"],["of","O"],["office","O"],["from","RANGE_START"],["Dec","MONTH"],["22","DOM"],["until","RANGE_END"],["Jan","MONTH"],["2","DOM"]]}
{"text": "gym every Mon Wed and Fri at 6am", "tokens": [["gym","O"],["every","RECUR"],["Mon","WEEKDAY"],["Wed","WEEKDAY"],["and","JOIN"],["Fri","WEEKDAY"],["at","GLUE"],["6","HOUR"],["am","MERIDIEM"]]}
{"text": "call mom in 20 minutes for half an hour", "tokens": [["call","O"],["mom","O"],["in","GLUE"],["20","DUR"],["minutes","UNIT"],["for","GLUE"],["half","DUR"],["an","GLUE"],["hour","UNIT"]]}
{"text": "let's catch up next week, nothing urgent", "tokens": [["let's","O"],["catch","O"],["up","O"],["next","DEICTIC"],["week","UNIT"],[",","GLUE"],["nothing","O"],["urgent","O"]]}
{"text": "the party is on Christmas Eve at 7:30pm", "tokens": [["the","O"],["party","O"],["is","O"],["on","GLUE"],["Christmas","HOLIDAY"],["Eve","HOLIDAY"],["at","GLUE"],["7","HOUR"],[":","GLUE"],["30","MINUTE"],["pm","MERIDIEM"]]}
```

Study these until the conventions are automatic, then generate new sentences
in the same shape.

### What to generate (30,000 sentences total)

| # | Family | Count | Examples of the space |
|---|--------|-------|----------------------|
| 1 | Single date (relative) | 2,000 | today/tomorrow/yesterday/(the) day after tomorrow/(the) day before yesterday + optional clock |
| 2 | Single date (calendar) | 3,000 | month+day, day+month, month+day+year, 4-digit year alone, day-of-month alone with month context |
| 3 | Single weekday | 1,500 | friday, next/last/this Friday, friday next week, next week friday |
| 4 | Weekday lists and ranges | 1,500 | mon and wed, mon-wed, weekdays, weekends, every other Friday |
| 5 | Clocks | 2,000 | 3pm, 7:30am, eight pm, noon, midnight, quarter past 5, half past noon, morning/afternoon/evening/night |
| 6 | Deictic periods | 2,000 | next week/month/quarter/year, end of next month, start of this week, alone and qualified |
| 7 | Period-qualified dates | 2,500 | 24th august last year, 15th last month, friday next week, august next year, both word orders |
| 8 | Ranges | 2,000 | from X to Y (dates, clocks, mixed), between 2 and 4pm, through/until/till |
| 9 | Recurrences | 3,000 | every day/week/month/year, twice a week, 3 times a day, every weekday at 9, monthly on the 1st, weekly on Tuesdays |
| 10 | Ordinal-anchored | 1,500 | first/second/last Monday of next month, 2nd Friday each month |
| 11 | Durations and shifts | 2,000 | for 90 minutes, half an hour, in 20 minutes, 2 hours from now, a week before Christmas |
| 12 | Holidays | 1,000 | Christmas, New Year's Day, Thanksgiving, Independence Day, on/after/before them |
| 13 | Multi-expression sentences | 2,000 | two clauses (date + separate time, meeting A then B), join them with and/then/, |
| 14 | Mid-sentence / prose-wrapped | 2,000 | realistic carrier text before/after/around any family above (emails, chats, reminders) |
| 15 | Negatives (no time at all) | 1,000 | sentences about scheduling that contain no time expression — all O |

Rules of composition:
- Prefer natural, real-world register (emails, chat messages, calendar
  invites, voice-assistant phrasing) over textbook examples.
- Vary casing naturally but do not simulate typos or unusual whitespace.
- Keep sentences under 40 tokens.
- Do not repeat the same sentence; vary nouns, names, and structure.
- Do not use these reserved carrier phrases anywhere (they are reserved for
  evaluation): "could you arrange a reminder for", "our rehearsal begins
  at", "the train leaves at", "please put this in my diary for", "allow
  extra time", "the workshop continues".

Output: JSONL, one object per line, exactly the shape of the worked
examples. No commentary, no code fences, just the lines.



## MULTILINGUAL ADDENDUM (Hindi + Hinglish)

Same task, same labels, same conventions — three languages. Generate
everything above in English, plus the two sections below. One model is
trained on the combined corpus, so keep the label semantics IDENTICAL across
scripts: `कल` and `kal` are REL_DAY exactly like `tomorrow`.

### Hindi (Devanagari script) — 30,000 sentences

Vocabulary mapping (extend naturally, do not limit yourself to this list):

| Role | Hindi |
|------|-------|
| REL_DAY | आज (today), कल (yesterday OR tomorrow — see below), परसों (±2 days) |
| WEEKDAY | सोमवार, मंगलवार, बुधवार, गुरुवार, शुक्रवार, शनिवार, रविवार |
| DEICTIC | इस/यह (this), अगला/अगले/अगली (next), पिछला/पिछले/पिछली (last) — they inflect for gender/number |
| UNIT | दिन, हफ़्ते/सप्ताह, महीने/महीना, साल/वर्ष, तिमाही (quarter) |
| MONTH | जनवरी…दिसंबर (English months in Devanagari are common) |
| DAYPART | सुबह (morning), दोपहर (afternoon), शाम (evening), रात (night) |
| MERIDIEM | बजे (o'clock, after an hour number), सुबह/रात after an hour also marks time of day |
| CLOCK_OFFSET | सवा (quarter past: सवा चार = 4:15), पौने (quarter to: पौने पाँच = 4:45), डेढ़ (1:30), ढाई (2:30), आधा/आधे (half) |
| NUM | एक दो तीन चार पाँच छह सात आठ नौ दस ग्यारह बारह, plus digits |
| RECUR | हर, हरेक, रोज़/roz (daily, alone or after हर/har), ... से हर हफ़्ते |
| RANGE_START / RANGE_END | से / तक (from / until) |
| HOLIDAY | दिवाली, होली, ईद, गणेश चतुर्थी, स्वतंत्रता दिवस |
| GLUE | को (to/at), में (in), पर (on), का/की/के (of), और (and → JOIN) |

Ambiguity rule for कल / परसों: Hindi leaves yesterday/tomorrow to context.
Always write sentences where surrounding words disambiguate (आएगा/आना है for
future, आया था/गया था for past) and label कल REL_DAY either way. Do not
invent a separate label.

Devanagari conventions:
- Write digits as ASCII digits (8, 30), not Devanagari numerals.
- One token per whitespace-separated word in your output; matras and
  conjuncts stay inside the word token (हफ़्ते is ONE token, never split it).
- Example:

```json
{"text": "कल शाम को आठ बजे मीटिंग है", "tokens": [["कल","REL_DAY"],["शाम","DAYPART"],["को","GLUE"],["आठ","HOUR"],["बजे","MERIDIEM"],["मीटिंग","O"],["है","O"]]}
{"text": "अगले हफ़्ते सोमवार को जमा करना", "tokens": [["अगले","DEICTIC"],["हफ़्ते","UNIT"],["सोमवार","WEEKDAY"],["को","GLUE"],["जमा","O"],["करना","O"]]}
```

Cover the same 15 families and counts as the English table, in natural Hindi
register (WhatsApp messages, reminders, office chat) — not textbook
translations.

### Hinglish (Hindi in Latin script) — 30,000 sentences

Romanized Hindi mixed freely with English, the way people actually text:
"kal shaam ko 8 baje", "agla monday ko meeting", "har hafte Tuesday ko gym".
Same vocabulary as the Hindi table, romanized (kal, aaj, parso, somvaar,
agle/pichhle hafte, mahine, saal, subah, dopahar, shaam, raat, baje, har,
se/tak). Ambiguity rule for kal/parso is the same. Example:

```json
{"text": "kal shaam ko 8 baje call kar dena", "tokens": [["kal","REL_DAY"],["shaam","DAYPART"],["ko","GLUE"],["8","HOUR"],["baje","MERIDIEM"],["call","O"],["kar","O"],["dena","O"]]}
```

Embrace code-switching: English nouns inside Hindi structure ("agle week ka
plan banate hain"), English verbs with Hindi auxiliaries, and fully English
sentences are all fine. This section should feel like real chat, not
transliteration practice.

---

## Receiving the data on our side

1. Save the model's output as `packages/training/data/llm/raw.jsonl`.
2. Validate: `uv run --project packages/training python packages/training/torch/validate_llm_corpus.py`
   (reconstructs text from tokens, checks the label set, the digit-suffix
   and GLUE conventions, compilable schedules, reserved-phrase exclusion,
   and fingerprint dedupe). Inspect the rejection report; a good batch keeps
   >90%.
3. Convert + featurize into training bins alongside the structural corpora.
   LLM data enters training as a minority share (≤25%) with the structural
   corpus as the anchor.
