/**
 * jev-desk in a browser.
 *
 * The solver owns the numbers, Jev owns the judgment, the schema owns the
 * contract — and this page owns none of the three. Everything on it came from
 * one run of the same code the CLI runs, read back over a typed API.
 */

import { useState } from "react";
import type { Domain, RunView } from "./api/types";
import { AuditPanel } from "./components/AuditPanel";
import { BatteryPanel } from "./components/BatteryPanel";
import { CataloguePanel } from "./components/CataloguePanel";
import { FxPanel } from "./components/FxPanel";
import { ReplayPanel } from "./components/ReplayPanel";
import { ReportPanel } from "./components/ReportPanel";
import { RunLauncher } from "./components/RunLauncher";
import { RunList } from "./components/RunList";
import { Banner, Card, Empty, Stat, StatGrid, Tabs } from "./components/ui";
import { ms, num } from "./format";
import { useRun, useRunList, useServerInfo } from "./hooks/useApi";

type TabId = "fx" | "battery" | "replays" | "audit" | "report" | "catalogue";

export default function App() {
  const { info, error: infoError } = useServerInfo();
  const { runs, refresh } = useRunList();
  const [selected, setSelected] = useState<string | null>(null);
  const { run, error: runError } = useRun(selected);
  const [tab, setTab] = useState<TabId>("fx");

  const started = (view: RunView) => {
    setSelected(view.id);
    setTab(view.spec.domain === "battery" ? "battery" : "fx");
    refresh();
  };

  return (
    <div className="app">
      <header className="masthead">
        <h1>jev-desk</h1>
        <span className="tagline">
          the solver owns the numbers, Jev owns the judgment, the schema owns the contract
        </span>
        <span className="spacer" />
        {info && (
          <span className="badge">
            {info.default_mock ? "offline backend" : "system one"} · v{info.version}
          </span>
        )}
      </header>

      <div className="body">
        <aside className="sidebar">
          <RunLauncher info={info} onStarted={started} />
          <div>
            <div className="card-title">
              <h3>runs</h3>
              <span className="note">{runs.length}</span>
            </div>
            <RunList
              runs={runs}
              selected={selected}
              onSelect={setSelected}
              onChanged={() => {
                refresh();
                setSelected(null);
              }}
            />
          </div>
        </aside>

        <main className="main">
          {infoError && <Banner kind="bad">{infoError}</Banner>}
          {!selected && <Welcome />}
          {runError && <Banner kind="bad">{runError}</Banner>}
          {selected && run && <RunPanel run={run} tab={tab} onTab={setTab} />}
        </main>
      </div>
    </div>
  );
}

function Welcome() {
  return (
    <div className="stack">
      <Card title="what this is">
        <p className="muted" style={{ marginTop: 0 }}>
          Two desks — spot forex and battery energy trading — share one set of six typed judgment
          primitives. The deterministic side produces every number: prices, sizes, stops,
          schedules, P&amp;L. Jev never produces one. It classifies, scores, gates, checks, ranks
          or explains, and the result is a struct validated before it reaches a caller.
        </p>
        <p className="muted">
          Start a run on the left. Everything you see afterwards came from that run: the two books
          side by side, each named check held up against the outcomes it flagged, one day replayed
          with a single fact changed, and every typed call behind all of it.
        </p>
        <p className="dim" style={{ marginBottom: 0, fontSize: 12.5 }}>
          Synthetic data only. No brokers, no market connections, nothing that touches a real
          venue.
        </p>
      </Card>
    </div>
  );
}

function RunPanel({
  run,
  tab,
  onTab,
}: {
  run: RunView;
  tab: TabId;
  onTab: (id: TabId) => void;
}) {
  if (run.status === "running") {
    return (
      <Card title={`${run.id} — judging`}>
        <p className="muted">
          <span className="spin" /> {num(run.calls)} typed calls so far. Each decision is four
          calls on the forex desk and five on the battery desk, so a full 90-day run is a few
          hundred.
        </p>
      </Card>
    );
  }

  if (run.status === "failed" || !run.result) {
    return <Banner kind="bad">{run.error ?? "the run produced no result."}</Banner>;
  }

  const { fx, battery, compliance, cost } = run.result;
  const reportDomains: Domain[] = [
    ...(fx ? (["fx"] as Domain[]) : []),
    ...(battery ? (["battery"] as Domain[]) : []),
  ];

  const tabs = [
    ...(fx ? [{ id: "fx" as const, label: "forex" }] : []),
    ...(battery ? [{ id: "battery" as const, label: "battery" }] : []),
    { id: "replays" as const, label: "replays" },
    { id: "report" as const, label: "report.md" },
    { id: "audit" as const, label: "audit", badge: num(cost.calls) },
    { id: "catalogue" as const, label: "the contract" },
  ];
  const active = tabs.some((t) => t.id === tab) ? tab : (tabs[0]?.id ?? "catalogue");

  return (
    <div className="stack">
      <StatGrid>
        <Stat label="run" value={run.id} sub={`seed ${run.spec.seed}`} />
        <Stat
          label="window"
          value={`${run.spec.days}d`}
          sub={run.spec.limit ? `judged ${run.spec.limit}` : "judged in full"}
        />
        <Stat label="backend" value={run.spec.mock ? "offline" : "system one"} />
        <Stat label="typed calls" value={num(cost.calls)} sub={`${num(cost.tokens)} tokens`} />
        <Stat
          label="judgment time"
          value={ms(cost.latency_ms)}
          sub={run.spec.mock ? "rules, not a model" : "wall clock"}
        />
      </StatGrid>

      {compliance && (
        <Card title="the day, for compliance" note="one explain call over both desks">
          <p className="muted" style={{ margin: 0 }}>
            {compliance.summary}
          </p>
        </Card>
      )}

      <Tabs tabs={tabs} active={active} onChange={onTab} />

      {active === "fx" && fx && <FxPanel runId={run.id} result={fx} />}
      {active === "battery" && battery && <BatteryPanel runId={run.id} result={battery} />}
      {active === "replays" && (
        <ReplayPanel fx={fx?.replay ?? null} battery={battery?.replay ?? null} />
      )}
      {active === "report" &&
        (reportDomains.length > 0 ? (
          <ReportPanel runId={run.id} domains={reportDomains} />
        ) : (
          <Empty>this run produced no report.</Empty>
        ))}
      {active === "audit" && <AuditPanel runId={run.id} calls={cost.calls} />}
      {active === "catalogue" && <CataloguePanel />}
    </div>
  );
}
