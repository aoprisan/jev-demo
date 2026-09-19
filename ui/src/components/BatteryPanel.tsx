/**
 * The battery desk.
 *
 * This is the one where the arrangement pays: the gated desk gives up a third
 * of the margin, eliminates every reserve breach, collects nine times the
 * capacity payment and wears the asset a little over half as hard. The table
 * puts both desks side by side rather than reporting the flattering half.
 */

import { useState } from "react";
import { getBatteryDay } from "../api/client";
import type { BatteryBook, BatteryResult } from "../api/types";
import { num, pct, signed, usd, words } from "../format";
import { useFetch } from "../hooks/useApi";
import { Curve, SchedulePlot } from "./charts";
import { ClassifyCard, GateCard, RankCard, ScoreCard } from "./judgment";
import {
  ActionBadge,
  BarList,
  Banner,
  Card,
  CheckList,
  Drawer,
  Empty,
  KeyValue,
  Stat,
  StatGrid,
} from "./ui";

function BookRow({ label, book }: { label: string; book: BatteryBook }) {
  return (
    <tr>
      <td>{label}</td>
      <td>{num(book.days_run)}</td>
      <td className={book.margin >= 0 ? "good" : "bad"}>{signed(book.margin)}</td>
      <td>{num(book.reserve_payment)}</td>
      <td className={book.reserve_breaches > 0 ? "bad" : "good"}>
        {num(book.reserve_breaches)}
        {book.days_with_breach > 0 && (
          <span className="dim"> on {num(book.days_with_breach)}d</span>
        )}
      </td>
      <td>{num(book.cycles, 1)}</td>
      <td>{num(book.margin_per_cycle, 1)}</td>
    </tr>
  );
}

export function BatteryPanel({ runId, result }: { runId: string; result: BatteryResult }) {
  const [open, setOpen] = useState<number | null>(null);
  const breachesRemoved = result.solver_only.reserve_breaches - result.gated.reserve_breaches;

  return (
    <div className="stack">
      <StatGrid>
        <Stat label="days judged" value={num(result.days_judged)} />
        <Stat
          label="interventions"
          value={num(result.interventions)}
          sub={`${pct(result.interventions / Math.max(1, result.days_judged))} of days`}
        />
        <Stat label="escalations" value={num(result.escalations)} sub="sent to a human" />
        <Stat label="flagged for review" value={num(result.reviews)} sub="thin certainty; gate stands" />
        <Stat label="failed checks" value={num(result.failed_checks)} />
        <Stat
          label="breaches removed"
          value={num(breachesRemoved)}
          tone={breachesRemoved > 0 ? "good" : "muted"}
          sub="reserve hours"
        />
      </StatGrid>

      <Card title="the two desks" note="one runs balanced every day; one runs what jev chose">
        <table>
          <thead>
            <tr>
              <th>desk</th>
              <th>days run</th>
              <th>margin</th>
              <th>reserve paid</th>
              <th>breaches</th>
              <th>cycles</th>
              <th>per cycle</th>
            </tr>
          </thead>
          <tbody>
            <BookRow label="solver-only" book={result.solver_only} />
            <BookRow label="gated" book={result.gated} />
          </tbody>
        </table>
        <div style={{ marginTop: 12 }}>
          <Banner kind="accent">
            Standing down is not free and the engine accounts for it: a hold forfeits the capacity
            payment too. The gated desk ends {signed(result.gated.margin - result.solver_only.margin)}{" "}
            against the solver-only desk, and earns {num(result.gated.margin_per_cycle, 1)} per
            cycle of wear against {num(result.solver_only.margin_per_cycle, 1)}.
          </Banner>
        </div>
      </Card>

      <div className="grid two">
        <Card title="cumulative margin" note="in day order">
          {result.margin_curve.length > 0 ? (
            <Curve
              series={[
                {
                  name: "solver-only",
                  values: result.margin_curve.map((p) => p.solver_only),
                  color: "#79b8ff",
                  dashed: true,
                },
                {
                  name: "gated",
                  values: result.margin_curve.map((p) => p.gated),
                  color: "#6fd6bb",
                },
              ]}
              xLabel={`${result.margin_curve.length} days`}
            />
          ) : (
            <Empty>no days judged.</Empty>
          )}
        </Card>

        <Card title="what jev chose" note="the ranking's first place, and the gate">
          <BarList
            rows={result.rank_distribution.map((row) => ({
              label: row.schedule,
              value: row.count,
            }))}
            total={Math.max(1, result.days_judged)}
          />
          <div style={{ marginTop: 14 }}>
            <BarList
              rows={result.gate_distribution.map((row) => ({
                label: row.action,
                value: row.count,
              }))}
              total={Math.max(1, result.days_judged)}
            />
          </div>
        </Card>
      </div>

      <Card title="days" note="click a row for all three schedules and every stage">
        <div className="table-scroll">
          <table>
            <thead>
              <tr>
                <th>date</th>
                <th>regime</th>
                <th>chose</th>
                <th>risk</th>
                <th>checks</th>
                <th>gate</th>
                <th>solver</th>
                <th>gated</th>
                <th>delta</th>
                <th>breaches</th>
              </tr>
            </thead>
            <tbody>
              {result.days.map((row) => (
                <tr
                  key={row.day}
                  className={`clickable ${open === row.day ? "selected" : ""}`}
                  onClick={() => setOpen(row.day)}
                >
                  <td>{row.date}</td>
                  <td className="dim">{row.regime}</td>
                  <td>{words(row.chosen)}</td>
                  <td>{row.risk_score}</td>
                  <td className={row.checks_failed.length > 0 ? "warn" : "dim"}>
                    {row.checks_failed.length > 0 ? row.checks_failed.length : "—"}
                  </td>
                  <td>
                    <ActionBadge action={row.action} size={row.size_factor} />
                  </td>
                  <td>{signed(row.solver_margin)}</td>
                  <td>{signed(row.gated_margin)}</td>
                  <td className={row.margin_delta >= 0 ? "good" : "bad"}>
                    {signed(row.margin_delta)}
                  </td>
                  <td>
                    <span className={row.solver_breaches > 0 ? "bad" : "dim"}>
                      {row.solver_breaches}
                    </span>
                    <span className="dim"> / </span>
                    <span className={row.gated_breaches > 0 ? "bad" : "good"}>
                      {row.gated_breaches}
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Card>

      {open !== null && (
        <BatteryDayDrawer runId={runId} day={open} onClose={() => setOpen(null)} />
      )}
    </div>
  );
}

/** One day in full: the three schedules the solver offered, and all five stages. */
function BatteryDayDrawer({
  runId,
  day,
  onClose,
}: {
  runId: string;
  day: number;
  onClose: () => void;
}) {
  const { data, error, loading } = useFetch(() => getBatteryDay(runId, day), `${runId}/${day}`);

  return (
    <Drawer title={`day ${day}`} onClose={onClose}>
      {loading && <Empty>reading the day…</Empty>}
      {error && <Banner kind="bad">{error}</Banner>}
      {data && (
        <div className="stack">
          <Card title="the three schedules" note="all valid; the solver owns every number">
            <table>
              <thead>
                <tr>
                  <th>schedule</th>
                  <th>expected</th>
                  <th>cycles</th>
                  <th>degradation</th>
                  <th>min soc</th>
                </tr>
              </thead>
              <tbody>
                {data.schedules.map((schedule) => (
                  <tr key={schedule.kind}>
                    <td className={schedule.kind === data.row.chosen ? "accent" : undefined}>
                      {words(schedule.kind)}
                    </td>
                    <td>{signed(schedule.expected_margin)}</td>
                    <td>{num(schedule.cycles, 2)}</td>
                    <td>{num(schedule.degradation_cost)}</td>
                    <td>{num(schedule.min_soc_mwh, 2)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            {data.schedules
              .filter((schedule) => schedule.kind === data.row.chosen)
              .map((schedule) => (
                <div key={schedule.kind} style={{ marginTop: 14 }}>
                  <p className="dim mono" style={{ fontSize: 11.5, margin: "0 0 6px" }}>
                    {words(schedule.kind)} — the one the ranking chose
                  </p>
                  <SchedulePlot powerMw={schedule.power_mw} socMwh={schedule.soc_mwh} />
                </div>
              ))}
          </Card>

          <ClassifyCard view={data.regime} title="stage 1 — regime" />
          <RankCard ranking={data.ranking} title="stage 2 — the ranking" />

          <Card title="stage 3 — sanity" note="check">
            <CheckList checks={data.checks} />
          </Card>

          <ScoreCard view={data.risk} title="stage 4 — operational risk" />
          <GateCard view={data.gate} title="stage 5 — the gate" />
          {data.review && (
            <Card title="flagged for review" note="code-side policy">
              <p className="reason">{data.review}</p>
            </Card>
          )}

          <Card
            title="what judging it cost"
            note="every call tagged with this decision"
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

          <Card title="what each desk ran" note="at intraday prices">
            <KeyValue
              rows={[
                [
                  "solver-only",
                  `${words(data.solver_only.schedule)} at ${pct(data.solver_only.size_factor)} — ${signed(data.solver_only.realised_margin)}, ${data.solver_only.reserve_breaches} breach hours`,
                ],
                [
                  "gated",
                  `${words(data.gated.schedule)} at ${pct(data.gated.size_factor)} — ${signed(data.gated.realised_margin)}, ${data.gated.reserve_breaches} breach hours`,
                ],
                ["reserve paid", `${num(data.gated.reserve_payment)} against ${num(data.solver_only.reserve_payment)}`],
                ["cycles", `${num(data.gated.cycles, 2)} against ${num(data.solver_only.cycles, 2)}`],
              ]}
            />
          </Card>
        </div>
      )}
    </Drawer>
  );
}
