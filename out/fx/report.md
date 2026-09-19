# Forex session

90 days from 2025-01-06, 3 pairs, seed `20250106`, backend `mock`. Bollinger(20, 2.0) mean reversion on 4h bars.

## Candidates

| pair | candidates | executed |
| --- | --: | --: |
| EUR/USD | 77 | 73 |
| GBP/USD | 64 | 62 |
| USD/JPY | 73 | 70 |

## Gate distribution

| action | count |
| --- | --: |
| execute | 85 |
| reduce | 120 |
| hold | 9 |
| escalate | 0 |

## Checks

| check | failed | of |
| --- | --: | --: |
| stop_sane | 73 | 214 |
| signal_valid_in_regime | 42 | 214 |
| correlated_exposure_ok | 21 | 214 |

The classifier agreed with the generator's regime on **59.3%** of 214 decisions. That regime is never sent to Jev; the comparison is made afterwards, which is why it is not 100%.

## Judgment scorecard

Each named check held up against what actually happened. The outcomes are the **ungated** fills, so a check is scored on its own merits rather than on the gate's response to it. A positive `edge` means the check flagged the worse decisions, which is what it is for.

| check | flagged | mean bp | hit | passed | mean bp | hit | edge |
| --- | --: | --: | --: | --: | --: | --: | --: |
| stop_sane | 73 | -3.7 | 37% | 141 | +27.2 | 55% | +30.9 |
| signal_valid_in_regime | 42 | +64.1 | 69% | 172 | +5.1 | 44% | -59.1 |
| correlated_exposure_ok | 21 | +27.5 | 57% | 193 | +15.5 | 48% | -12.0 |

> **`signal_valid_in_regime` is inverted in this market.** It flags decisions averaging +64.1 bp while passing ones averaging +5.1 bp, so acting on it costs money. The rule is implemented exactly as specified — an excursion outside the band should not be bought into a trend — and the regime call behind it is accurate. What fails is the inference: at a band excursion, the same sharpness that marks a trend also marks an overshoot that reverts hard, and the second effect dominates. This is the kind of thing a typed, logged judgment layer exists to make visible.

> **`correlated_exposure_ok` is inverted in this market.** It flags decisions averaging +27.5 bp while passing ones averaging +15.5 bp, so acting on it costs money. The rule is implemented exactly as specified — an excursion outside the band should not be bought into a trend — and the regime call behind it is accurate. What fails is the inference: at a band excursion, the same sharpness that marks a trend also marks an overshoot that reverts hard, and the second effect dominates. This is the kind of thing a typed, logged judgment layer exists to make visible.

## Books

|  | ungated | gated |
| --- | --: | --: |
| trades | 214 | 205 |
| P&L | +35,620 | +13,065 |
| return on notional | 16.65 bp | 9.70 bp |
| hit rate | 49% | 48% |
| max drawdown | 7,755 | 10,719 |
| notional deployed | 21,400,000 | 13,475,000 |

`ungated` runs every candidate at the size the solver chose. `gated` runs what the judgment layer allowed, on the same signals.

## Ten most consequential decisions

| when | pair | side | action | size | P&L effect | reason |
| --- | --: | --: | --: | --: | --: | --: |
| d078 08:00 | GBP/USD | long | reduce | 25% | -1,970 | GBP/USD long: reduce at 25% of size — signal_valid_in_regime failed (conf 0.45, next hold 0.31) |
| d078 04:00 | GBP/USD | long | reduce | 25% | -1,818 | GBP/USD long: reduce at 25% of size — signal_valid_in_regime failed (conf 0.45, next hold 0.31) |
| d036 12:00 | GBP/USD | long | hold | 0% | -1,664 | GBP/USD long: hold at 0% of size — risk 92, imminent_event (conf 0.45, next reduce 0.31) |
| d076 04:00 | USD/JPY | long | reduce | 25% | -1,599 | USD/JPY long: reduce at 25% of size — signal_valid_in_regime failed (conf 0.45, next hold 0.31) |
| d078 00:00 | GBP/USD | long | reduce | 25% | -1,590 | GBP/USD long: reduce at 25% of size — signal_valid_in_regime failed (conf 0.45, next hold 0.31) |
| d044 04:00 | GBP/USD | short | reduce | 25% | -1,472 | GBP/USD short: reduce at 25% of size — signal_valid_in_regime failed (conf 0.45, next hold 0.31) |
| d040 20:00 | EUR/USD | short | reduce | 25% | -1,466 | EUR/USD short: reduce at 25% of size — signal_valid_in_regime failed (conf 0.45, next hold 0.31) |
| d044 00:00 | GBP/USD | short | reduce | 25% | -1,332 | GBP/USD short: reduce at 25% of size — signal_valid_in_regime failed (conf 0.45, next hold 0.31) |
| d076 00:00 | USD/JPY | long | reduce | 25% | -1,327 | USD/JPY long: reduce at 25% of size — signal_valid_in_regime failed (conf 0.45, next hold 0.31) |
| d067 04:00 | EUR/USD | long | reduce | 25% | -1,313 | EUR/USD long: reduce at 25% of size — signal_valid_in_regime failed (conf 0.45, next hold 0.31) |

## Explain

**trader** — the forex session — the judgment layer changed 129 of 214 decisions without needing a human. The session was notable. Gating moved the book from +35,620 to +13,065, at 9.7 against 16.6 bp of return; 129 of 214 were held or sized down, and the hit rate went from 49% to 48% on 9 fewer trades.

**compliance** — the forex session — the judgment layer changed 129 of 214 decisions without needing a human. The session was notable. 129 of 214 were held or sized down; the same named checks ran on every one, 136 failing; all 856 typed calls are in decisions.jsonl, and the hit rate went from 49% to 48% on 9 fewer…


---

858 typed calls, 718,065 tokens, backend `mock`. Every call and its output is in `decisions.jsonl`.
