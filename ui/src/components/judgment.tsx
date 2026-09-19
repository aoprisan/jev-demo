/**
 * The six primitives, rendered.
 *
 * Every panel shows the same two things: the typed verdict, and the
 * distribution it was read off. The prose is composed from that verdict by
 * `jev-core` — it cannot assert anything the schema did not already carry, and
 * showing the numbers beside it is the way to make that visible.
 */

import type { ClassifyView, GateView, RankedView, ScoreView } from "../api/types";
import { pct, words } from "../format";
import { ActionBadge, BarList, Card, Meter } from "./ui";

function Distribution({ rows }: { rows: { label: string; p: number }[] }) {
  if (rows.length === 0) return null;
  return (
    <div style={{ marginTop: 10 }}>
      <BarList
        rows={rows.map((r) => ({ label: r.label, value: r.p }))}
        total={1}
        render={(v) => pct(v, 0)}
      />
    </div>
  );
}

export function ClassifyCard({ view, title }: { view: ClassifyView; title: string }) {
  return (
    <Card title={title} note="classify">
      <div className="mono" style={{ fontSize: 15 }}>
        {words(view.label)}
      </div>
      <div className="dim mono" style={{ fontSize: 11.5, marginBottom: 8 }}>
        confidence {pct(view.confidence, 0)}
      </div>
      <p className="reason">{view.reason}</p>
      <Distribution rows={view.distribution} />
    </Card>
  );
}

export function ScoreCard({ view, title }: { view: ScoreView; title: string }) {
  return (
    <Card title={title} note="score">
      <div className="mono" style={{ fontSize: 15 }}>
        {view.score}
        <span className="dim">/100</span>
      </div>
      <div style={{ margin: "6px 0 8px" }}>
        <Meter value={view.score / 100} />
      </div>
      <p className="reason">{view.reason}</p>
      {view.drivers.length > 0 && (
        <p className="dim mono" style={{ fontSize: 11.5, marginBottom: 0 }}>
          drivers: {view.drivers.map(words).join(", ")}
        </p>
      )}
      <Distribution rows={view.distribution} />
    </Card>
  );
}

export function GateCard({ view, title }: { view: GateView; title: string }) {
  return (
    <Card title={title} note="gate">
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <ActionBadge action={view.action} size={view.size_factor} />
        <span className="dim mono" style={{ fontSize: 11.5 }}>
          confidence {pct(view.confidence, 0)}
        </span>
      </div>
      <p className="reason" style={{ marginTop: 10 }}>
        {view.reason}
      </p>
      <Distribution rows={view.distribution} />
    </Card>
  );
}

/**
 * The whole ordering, read off one choice distribution — one call, N
 * candidates, and the margins come out with it.
 */
export function RankCard({ ranking, title }: { ranking: RankedView[]; title: string }) {
  return (
    <Card title={title} note="rank">
      <ol style={{ margin: 0, paddingLeft: 18, display: "grid", gap: 8 }}>
        {ranking.map((entry) => (
          <li key={entry.id}>
            <div className="mono">
              {words(entry.id)} <span className="dim">({pct(entry.p, 0)})</span>
            </div>
            <div className="note dim" style={{ fontSize: 11.5 }}>
              {entry.rationale}
            </div>
          </li>
        ))}
      </ol>
    </Card>
  );
}
