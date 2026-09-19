/** The runs this process is holding, newest first. */

import { deleteRun } from "../api/client";
import type { RunSummary } from "../api/types";
import { clock, num } from "../format";

export function RunList({
  runs,
  selected,
  onSelect,
  onChanged,
}: {
  runs: RunSummary[];
  selected: string | null;
  onSelect: (id: string) => void;
  onChanged: () => void;
}) {
  if (runs.length === 0) {
    return <p className="dim" style={{ fontSize: 12.5 }}>no runs yet.</p>;
  }

  const forget = async (event: React.MouseEvent, id: string) => {
    event.stopPropagation();
    await deleteRun(id).catch(() => undefined);
    onChanged();
  };

  return (
    <div>
      {runs.map((run) => (
        <button
          key={run.id}
          className="run"
          aria-current={run.id === selected}
          onClick={() => onSelect(run.id)}
        >
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <span className="id">{run.id}</span>
            {run.status === "running" && <span className="spin" />}
            {run.status === "failed" && <span className="bad mono">failed</span>}
            <span style={{ flex: 1 }} />
            <span
              className="dim mono"
              style={{ fontSize: 11 }}
              onClick={(event) => forget(event, run.id)}
              role="presentation"
              title="forget this run"
            >
              ✕
            </span>
          </div>
          <div className="meta">
            seed {run.spec.seed} · {run.spec.days}d · {run.spec.mock ? "mock" : "live"} ·{" "}
            {num(run.calls)} calls · {clock(run.started_at_ms)}
          </div>
        </button>
      ))}
    </div>
  );
}
