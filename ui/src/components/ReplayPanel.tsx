/**
 * The counterfactual replays.
 *
 * One day judged twice with exactly one thing changed. The prices are identical
 * across both runs, so whatever moves between the panes is the judgment layer
 * reacting to that one thing — and nothing else.
 */

import type { BatteryReplay, FxReplay } from "../api/types";
import { num, pct, words } from "../format";
import { Badge, Banner, Card, CheckList, Empty, KeyValue } from "./ui";
import { ActionBadge } from "./ui";

function Pane({
  label,
  rows,
  reason,
  children,
}: {
  label: string;
  rows: [string, React.ReactNode][];
  reason: string;
  children?: React.ReactNode;
}) {
  return (
    <Card title={label}>
      <KeyValue rows={rows} />
      <p className="reason" style={{ marginTop: 10 }}>
        {reason}
      </p>
      {children}
    </Card>
  );
}

export function FxReplayView({ replay }: { replay: FxReplay }) {
  const flipped = replay.with_event.gate.action !== replay.without_event.gate.action;
  return (
    <Card
      title={`replay — ${replay.pair} on ${replay.date}`}
      note="with the CPI release, and with it removed"
    >
      <p className="dim mono" style={{ fontSize: 11.5, marginTop: 0 }}>
        same bars, same candidate: {replay.pair} {replay.side} at {num(replay.price, 5)}, stop{" "}
        {num(replay.stop, 5)}, target {num(replay.target, 5)}
      </p>
      <div className="panes">
        {[replay.with_event, replay.without_event].map((pane) => (
          <Pane
            key={pane.label}
            label={pane.label}
            reason={pane.gate.reason}
            rows={[
              ["next release", pane.event],
              ["event risk", `${pane.risk_score}/100`],
              [
                "gate",
                <ActionBadge key="a" action={pane.gate.action} size={pane.gate.size_factor} />,
              ],
              ["size factor", pct(pane.gate.size_factor)],
              ["confidence", pct(pane.gate.confidence)],
            ]}
          />
        ))}
      </div>
      <div style={{ marginTop: 12 }}>
        <Banner kind={flipped ? "accent" : "warn"}>
          {flipped
            ? `the gate changed its mind: ${replay.with_event.gate.action} → ${replay.without_event.gate.action}, on the calendar alone.`
            : "the gate held to the same action; only the size and the reason moved."}
        </Banner>
      </div>
    </Card>
  );
}

export function BatteryReplayView({ replay }: { replay: BatteryReplay }) {
  return (
    <Card
      title={`replay — day ${replay.day} (${replay.date})`}
      note={`with and without a grid notice over ${String(replay.from_hour).padStart(2, "0")}:00–${String(replay.to_hour).padStart(2, "0")}:00`}
    >
      <p className="dim mono" style={{ fontSize: 11.5, marginTop: 0 }}>
        same prices, same three schedules; only the grid operator's note differs — “{replay.notice}”
      </p>
      <div className="panes">
        {[replay.quiet, replay.noticed].map((pane) => (
          <Pane
            key={pane.label}
            label={pane.label}
            reason={pane.gate.reason}
            rows={[
              ["chose", words(pane.chosen)],
              [
                "ranking",
                <span key="r" className="mono">
                  {pane.ranking.map((r, i) => `${i + 1}. ${words(r.id)} (fit ${r.fit.toFixed(2)})`).join("  ")}
                </span>,
              ],
              ["risk", `${pane.risk_score}/100`],
              [
                "reserve held",
                <Badge key="b" kind={pane.reserve_held ? "ok" : "fail"}>
                  {pane.reserve_held ? "yes" : "no"}
                </Badge>,
              ],
              [
                "gate",
                <ActionBadge key="a" action={pane.gate.action} size={pane.gate.size_factor} />,
              ],
            ]}
          >
            <div style={{ marginTop: 12 }}>
              <CheckList checks={pane.checks} />
            </div>
          </Pane>
        ))}
      </div>
      <div style={{ marginTop: 12 }}>
        <Banner kind={replay.flipped ? "accent" : "warn"}>
          {replay.flipped
            ? `the ranking flipped: ${words(replay.quiet.chosen)} → ${words(replay.noticed.chosen)}, on one sentence from the grid operator.`
            : "the ranking held: the notice did not change which schedule came first."}
        </Banner>
      </div>
    </Card>
  );
}

export function ReplayPanel({
  fx,
  battery,
}: {
  fx: FxReplay | null;
  battery: BatteryReplay | null;
}) {
  if (!fx && !battery) {
    return <Empty>this run produced no replay — try a longer window.</Empty>;
  }
  return (
    <div className="stack">
      {fx && <FxReplayView replay={fx} />}
      {battery && <BatteryReplayView replay={battery} />}
    </div>
  );
}
