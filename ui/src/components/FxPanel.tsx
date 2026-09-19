/**
 * The forex desk.
 *
 * The demo does not dress this one up: the gated book underperforms, and the
 * scorecard says why — one of the three named checks is inverted, flagging the
 * better trades. Being able to see that at all is the argument for the whole
 * arrangement, so it is on the page rather than in a footnote.
 */

import { useState } from "react";
import { getFxDecision } from "../api/client";
import type { FxBook, FxResult } from "../api/types";
import { bps, num, pct, signed, usd } from "../format";
import { useFetch } from "../hooks/useApi";
import { Curve } from "./charts";
import { ClassifyCard, GateCard, ScoreCard } from "./judgment";
import {
  ActionBadge,
  Badge,
  Banner,
  Card,
  CheckList,
  Drawer,
  Empty,
  KeyValue,
  Stat,
  StatGrid,
} from "./ui";

function BookRow({ label, book }: { label: string; book: FxBook }) {
  return (
    <tr>
      <td>{label}</td>
      <td>{num(book.trades)}</td>
      <td className={book.pnl >= 0 ? "good" : "bad"}>{signed(book.pnl)}</td>
      <td>{pct(book.hit_rate)}</td>
      <td>{num(book.max_drawdown)}</td>
      <td>{num(book.notional)}</td>
      <td className={book.return_bps >= 0 ? "good" : "bad"}>{bps(book.return_bps, 2)}</td>
    </tr>
  );
}

export function FxPanel({
  runId,
  result,
  onShowCalls,
}: {
  runId: string;
  result: FxResult;
  /** Open the audit log narrowed to one decision's calls. */
  onShowCalls?: (decisionId: string) => void;
}) {
  const [open, setOpen] = useState<number | null>(null);
  const inverted = result.scorecard.filter((card) => card.inverted);

  return (
    <div className="stack">
      <StatGrid>
        <Stat label="candidates judged" value={num(result.decisions_judged)} />
        <Stat
          label="gate interventions"
          value={num(result.interventions)}
          sub={`${pct(result.interventions / Math.max(1, result.decisions_judged))} of decisions`}
        />
        <Stat label="escalations" value={num(result.escalations)} sub="sent to a human" />
        <Stat label="flagged for review" value={num(result.reviews)} sub="thin certainty; gate stands" />
        <Stat label="failed checks" value={num(result.failed_checks)} />
        <Stat
          label="regime accuracy"
          value={pct(result.regime_accuracy)}
          sub="against hidden truth"
        />
      </StatGrid>

      <Card title="the two books" note="same signals, different sizes">
        <table>
          <thead>
            <tr>
              <th>book</th>
              <th>trades</th>
              <th>p&amp;l</th>
              <th>hit</th>
              <th>drawdown</th>
              <th>notional</th>
              <th>return</th>
            </tr>
          </thead>
          <tbody>
            <BookRow label="ungated" book={result.ungated} />
            <BookRow label="gated" book={result.gated} />
          </tbody>
        </table>
      </Card>

      <div className="grid two">
        <Card title="cumulative p&l" note="in trade order">
          {result.equity.length > 0 ? (
            <Curve
              series={[
                {
                  name: "ungated",
                  values: result.equity.map((p) => p.ungated),
                  color: "#79b8ff",
                  dashed: true,
                },
                { name: "gated", values: result.equity.map((p) => p.gated), color: "#6fd6bb" },
              ]}
              xLabel={`${result.equity.length} decisions`}
            />
          ) : (
            <Empty>no trades in this window.</Empty>
          )}
        </Card>

        <Card title="what the gate did" note="every candidate">
          <table>
            <thead>
              <tr>
                <th>action</th>
                <th>count</th>
                <th>share</th>
              </tr>
            </thead>
            <tbody>
              {result.gate_distribution.map((row) => (
                <tr key={row.action}>
                  <td>
                    <ActionBadge action={row.action} />
                  </td>
                  <td>{num(row.count)}</td>
                  <td className="dim">
                    {pct(row.count / Math.max(1, result.decisions_judged))}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      </div>

      <Card
        title="judgment scorecard"
        note="each named check against the ungated outcomes it flagged"
      >
        <table>
          <thead>
            <tr>
              <th>check</th>
              <th>flagged</th>
              <th>mean</th>
              <th>hit</th>
              <th>passed</th>
              <th>mean</th>
              <th>hit</th>
              <th>edge</th>
            </tr>
          </thead>
          <tbody>
            {result.scorecard.map((card) => (
              <tr key={card.name}>
                <td className={card.inverted ? "bad" : undefined}>{card.name}</td>
                <td>{num(card.failed_n)}</td>
                <td>{signed(card.failed_bps, 1)}</td>
                <td className="dim">{pct(card.failed_hit)}</td>
                <td>{num(card.passed_n)}</td>
                <td>{signed(card.passed_bps, 1)}</td>
                <td className="dim">{pct(card.passed_hit)}</td>
                <td className={card.edge_bps >= 0 ? "good" : "bad"}>{bps(card.edge_bps, 1)}</td>
              </tr>
            ))}
          </tbody>
        </table>
        {inverted.length > 0 && (
          <div style={{ marginTop: 12 }}>
            <Banner>
              <strong className="bad">{inverted.map((c) => c.name).join(", ")}</strong> is
              inverted: it flags the <em>better</em> trades, so acting on it costs money. The rule
              was left as specified and the result reported, rather than tuned until the P&amp;L
              flattered the pitch.
            </Banner>
          </div>
        )}
      </Card>

      <Card
        title="decisions"
        note="click a row for the features, the four stages and both fills"
      >
        <div className="table-scroll">
          <table>
            <thead>
              <tr>
                <th>date</th>
                <th>pair</th>
                <th>side</th>
                <th>regime</th>
                <th>risk</th>
                <th>checks</th>
                <th>gate</th>
                <th>ungated</th>
                <th>gated</th>
                <th>delta</th>
              </tr>
            </thead>
            <tbody>
              {result.decisions.map((row) => (
                <tr
                  key={row.index}
                  className={`clickable ${open === row.index ? "selected" : ""}`}
                  onClick={() => setOpen(row.index)}
                >
                  <td>
                    {row.date} <span className="dim">{String(row.hour).padStart(2, "0")}h</span>
                  </td>
                  <td>{row.pair}</td>
                  <td className="dim">{row.side}</td>
                  <td className={row.regime_correct ? undefined : "dim"}>{row.regime}</td>
                  <td>{row.risk_score}</td>
                  <td className={row.checks_failed.length > 0 ? "warn" : "dim"}>
                    {row.checks_failed.length > 0 ? row.checks_failed.length : "—"}
                  </td>
                  <td>
                    <ActionBadge action={row.action} size={row.size_factor} />
                  </td>
                  <td className={row.ungated_pnl !== null && row.ungated_pnl < 0 ? "bad" : ""}>
                    {row.ungated_pnl === null ? "—" : signed(row.ungated_pnl)}
                  </td>
                  <td className={row.gated_pnl !== null && row.gated_pnl < 0 ? "bad" : ""}>
                    {row.gated_pnl === null ? "—" : signed(row.gated_pnl)}
                  </td>
                  <td className={row.pnl_delta >= 0 ? "good" : "bad"}>{signed(row.pnl_delta)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Card>

      {open !== null && (
        <FxDecisionDrawer
          runId={runId}
          index={open}
          onClose={() => setOpen(null)}
          onShowCalls={onShowCalls}
        />
      )}
    </div>
  );
}

/** One candidate in full: what Jev read, what each stage returned, what it cost. */
function FxDecisionDrawer({
  runId,
  index,
  onClose,
  onShowCalls,
}: {
  runId: string;
  index: number;
  onClose: () => void;
  onShowCalls?: (decisionId: string) => void;
}) {
  const { data, error, loading } = useFetch(
    () => getFxDecision(runId, index),
    `${runId}/${index}`,
  );

  return (
    <Drawer
      title={`decision ${index}`}
      actions={
        data &&
        onShowCalls && (
          <button className="btn ghost" onClick={() => onShowCalls(data.row.decision_id)}>
            jev calls ({num(data.row.calls)}) →
          </button>
        )
      }
      onClose={onClose}
    >
      {loading && <Empty>reading the decision…</Empty>}
      {error && <Banner kind="bad">{error}</Banner>}
      {data && (
        <div className="stack">
          <Card title="the proposal" note="every number here is the solver's">
            <KeyValue
              rows={[
                ["pair", `${data.row.pair} ${data.row.side}`],
                ["when", `${data.row.date} ${String(data.row.hour).padStart(2, "0")}:00`],
                ["entry", num(data.row.price, 5)],
                ["stop", num(data.row.stop, 5)],
                ["target", num(data.row.target, 5)],
                ["size", `${num(data.row.size_units)}k units`],
              ]}
            />
          </Card>

          <ClassifyCard view={data.regime} title="stage 1 — regime" />

          <Card title="stage 2 — sanity" note="check">
            <CheckList checks={data.checks} />
          </Card>

          <ScoreCard view={data.risk} title="stage 3 — event risk" />
          <GateCard view={data.gate} title="stage 4 — the gate" />
          {data.review && (
            <Card title="flagged for review" note="code-side policy">
              <p className="reason">{data.review}</p>
            </Card>
          )}

          <Card
            title="what judging it cost"
            note="every call tagged with this decision"
            right={
              onShowCalls && (
                <button className="btn ghost" onClick={() => onShowCalls(data.row.decision_id)}>
                  see the {num(data.row.calls)} calls →
                </button>
              )
            }
          >
            <KeyValue
              rows={[
                ["decision id", data.row.decision_id],
                ["jev calls", num(data.row.calls)],
                ["tokens", num(data.row.tokens)],
                ["est. cost", usd(data.row.est_usd)],
              ]}
            />
          </Card>

          <Card
            title="what the judgment layer read"
            note="observable features only — no ground truth"
          >
            <KeyValue
              rows={[
                [
                  "next release",
                  data.features.next_event_kind && data.features.hours_to_event !== null
                    ? `${data.features.next_event_kind} in ${num(data.features.hours_to_event, 1)}h`
                    : "none scheduled",
                ],
                ["stop / atr", num(data.features.stop_atr_multiple, 2)],
                ["stop clears noise", data.features.stop_clears_noise ? "yes" : "no"],
                ["reward : risk", num(data.features.reward_risk, 2)],
                ["trend strength", num(data.features.trend_strength, 2)],
                ["volatility pct", pct(data.features.volatility_percentile)],
                ["band excursion", num(data.features.band_excursion, 2)],
                ["exposure multiple", num(data.features.exposure_multiple, 2)],
                ["stress", num(data.features.stress_indicator, 2)],
              ]}
            />
            {data.headlines.length > 0 && (
              <ul className="dim" style={{ fontSize: 11.5, margin: "10px 0 0", paddingLeft: 16 }}>
                {data.headlines.map((headline) => (
                  <li key={headline}>{headline}</li>
                ))}
              </ul>
            )}
          </Card>

          <Card title="outcome" note="the generator's regime is attached after the fact">
            <KeyValue
              rows={[
                [
                  "true regime",
                  <span key="t">
                    {data.row.true_regime}{" "}
                    <Badge kind={data.row.regime_correct ? "ok" : "fail"}>
                      {data.row.regime_correct ? "classifier agreed" : "classifier differed"}
                    </Badge>
                  </span>,
                ],
                [
                  "ungated",
                  data.ungated
                    ? `${signed(data.ungated.pnl)} (${bps(data.ungated.pnl_bps)}, ${data.ungated.exit})`
                    : "no fill",
                ],
                [
                  "gated",
                  data.gated
                    ? `${signed(data.gated.pnl)} (${bps(data.gated.pnl_bps)}, ${data.gated.exit})`
                    : "stood down",
                ],
              ]}
            />
          </Card>
        </div>
      )}
    </Drawer>
  );
}
