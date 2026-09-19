# Battery session

90 days from 2025-01-06, a 1 MW / 2 MWh battery, seed `20250106`, backend `mock`.

## Candidate schedules

| schedule | mean margin | mean cycles | ranked first |
| --- | --: | --: | --: |
| aggressive | 131.9 | 1.82 | 0 |
| balanced | 123.0 | 1.42 | 42 |
| reserve_heavy | 99.5 | 1.10 | 48 |

All three respect the state-of-charge bounds. They differ in how much headroom they keep and how hard they cycle.

## Ranking

| schedule | ranked first |
| --- | --: |
| aggressive | 0 |
| balanced | 42 |
| reserve_heavy | 48 |

## Gate distribution

| action | days |
| --- | --: |
| execute | 46 |
| reduce | 25 |
| hold | 13 |
| escalate | 6 |

## Checks

| check | failed | of |
| --- | --: | --: |
| reserve_ok | 0 | 90 |
| margin_plausible | 6 | 90 |
| cycle_budget_ok | 27 | 90 |

`reserve_ok` rarely fails because the ranking has already moved the day onto a schedule that holds the reserve. The check is what would catch it if it had not.

## Books

|  | solver-only | gated |
| --- | --: | --: |
| days run | 90 | 71 |
| days stood down | 0 | 19 |
| realised margin | +11,311 | +7,155 |
| reserve payments | 87 | 812 |
| reserve breaches | 77 h on 30 days | 0 h on 0 days |
| equivalent cycles | 127.6 | 71.1 |
| margin per cycle | 88.7 | 100.7 |
| max drawdown | 0 | 0 |

`solver-only` runs the balanced schedule every day, whole. `gated` runs whichever schedule the ranking chose, at the size the gate allowed, on the same prices.

## Ten most consequential decisions

| date | chose | action | size | margin effect | reason |
| --- | --: | --: | --: | --: | --: |
| 2025-02-15 | balanced | escalate | 0% | -231 | the balanced schedule for day 40: escalate at 0% of size — risk 80, cycles_nearly_spent (p=0.57, conf 0.45; next hold 0.31) |
| 2025-02-09 | balanced | reduce | 50% | -191 | the balanced schedule for day 34: reduce at 50% of size — cycle_budget_ok failed (p=0.57, conf 0.45; next hold 0.31) |
| 2025-02-16 | balanced | escalate | 0% | -186 | the balanced schedule for day 41: escalate at 0% of size — risk 80, price_dislocation (p=0.57, conf 0.45; next hold 0.31) |
| 2025-03-13 | balanced | hold | 0% | -140 | the balanced schedule for day 66: hold at 0% of size (p=0.57, conf 0.45; next reduce 0.31) |
| 2025-03-12 | reserve_heavy | reduce | 50% | -132 | the reserve_heavy schedule for day 65: reduce at 50% of size — cycle_budget_ok failed (p=0.57, conf 0.45; next hold 0.31) |
| 2025-02-21 | reserve_heavy | hold | 0% | -125 | the reserve_heavy schedule for day 46: hold at 0% of size (p=0.57, conf 0.45; next reduce 0.31) |
| 2025-03-14 | reserve_heavy | escalate | 0% | -124 | the reserve_heavy schedule for day 67: escalate at 0% of size — risk 80, price_dislocation (p=0.57, conf 0.45; next hold 0.31) |
| 2025-02-22 | reserve_heavy | hold | 0% | -119 | the reserve_heavy schedule for day 47: hold at 0% of size (p=0.57, conf 0.45; next reduce 0.31) |
| 2025-03-26 | reserve_heavy | hold | 0% | -119 | the reserve_heavy schedule for day 79: hold at 0% of size (p=0.57, conf 0.45; next reduce 0.31) |
| 2025-03-23 | balanced | reduce | 50% | -118 | the balanced schedule for day 76: reduce at 50% of size — cycle_budget_ok failed (p=0.57, conf 0.45; next hold 0.31) |

## Explain

**trader** — the battery session — 6 decision(s) went to a human because the state did not hold together. The session was difficult. The reserve was missed 0 times against 77 without the judgment layer; 72 of 90 were held or sized down, and 6 went to a human rather than being sized down.

**compliance** — the battery session — 6 decision(s) went to a human because the state did not hold together. The session was difficult. 72 of 90 were held or sized down; the same named checks ran on every one, 33 failing, and all 450 typed calls are in decisions.jsonl.


---

452 typed calls, 585,864 tokens, backend `mock`. Every call and its output is in `decisions.jsonl`.
