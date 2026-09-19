/**
 * The audit log.
 *
 * One row per typed call: the state that was judged, the questions asked, the
 * verdicts returned and the output composed from them. This is the artefact the
 * whole arrangement exists to produce, so it is shown raw rather than
 * summarised.
 */

import { useState } from "react";
import { decisionsUrl, getCall, getCalls } from "../api/client";
import { clock, ms, num } from "../format";
import { useFetch } from "../hooks/useApi";
import { Banner, Card, Drawer, Empty } from "./ui";

const PAGE = 100;

export function AuditPanel({ runId, calls }: { runId: string; calls: number }) {
  const [offset, setOffset] = useState(0);
  const [open, setOpen] = useState<number | null>(null);
  const { data, error } = useFetch(
    () => getCalls(runId, offset, PAGE),
    `${runId}/${offset}/${calls}`,
  );

  if (error) return <Banner kind="bad">{error}</Banner>;
  if (!data) return <Empty>reading the audit log…</Empty>;
  if (data.total === 0) return <Empty>no calls recorded yet.</Empty>;

  const last = Math.min(offset + PAGE, data.total);

  return (
    <div className="stack">
      <Card
        title="typed calls"
        note={`${num(data.total)} in this run`}
        right={
          <a className="btn ghost" href={decisionsUrl(runId)} download>
            decisions.jsonl
          </a>
        }
      >
        <div className="table-scroll">
          <table>
            <thead>
              <tr>
                <th>#</th>
                <th>primitive</th>
                <th>stage</th>
                <th>questions</th>
                <th>backend</th>
                <th>tokens</th>
                <th>latency</th>
                <th>at</th>
              </tr>
            </thead>
            <tbody>
              {data.calls.map((call) => (
                <tr
                  key={call.index}
                  className={`clickable ${open === call.index ? "selected" : ""}`}
                  onClick={() => setOpen(call.index)}
                >
                  <td className="dim">{call.index}</td>
                  <td>{call.primitive}</td>
                  <td className="dim">{call.stage ?? "—"}</td>
                  <td>{call.asks}</td>
                  <td className="dim">{call.backend}</td>
                  <td>
                    {num((call.input_tokens ?? 0) + (call.output_tokens ?? 0))}
                  </td>
                  <td className="dim">{ms(call.latency_ms)}</td>
                  <td className="dim">{clock(call.at_ms)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div style={{ display: "flex", gap: 8, alignItems: "center", marginTop: 10 }}>
          <button
            className="btn ghost"
            disabled={offset === 0}
            onClick={() => setOffset(Math.max(0, offset - PAGE))}
          >
            ← previous
          </button>
          <span className="dim mono" style={{ fontSize: 11.5 }}>
            {offset + 1}–{last} of {num(data.total)}
          </span>
          <button
            className="btn ghost"
            disabled={last >= data.total}
            onClick={() => setOffset(offset + PAGE)}
          >
            next →
          </button>
        </div>
      </Card>

      {open !== null && <CallDrawer runId={runId} index={open} onClose={() => setOpen(null)} />}
    </div>
  );
}

function CallDrawer({
  runId,
  index,
  onClose,
}: {
  runId: string;
  index: number;
  onClose: () => void;
}) {
  const { data, error, loading } = useFetch(() => getCall(runId, index), `${runId}/call/${index}`);

  return (
    <Drawer title={`call ${index}`} onClose={onClose}>
      {loading && <Empty>reading the call…</Empty>}
      {error && <Banner kind="bad">{error}</Banner>}
      {data && (
        <div className="stack">
          <Card title="the call">
            <pre>
              {data.primitive}
              {data.stage ? ` · stage ${data.stage}` : ""} · {data.backend} · {data.model} ·{" "}
              {ms(data.latency_ms)} · {num((data.input_tokens ?? 0) + (data.output_tokens ?? 0))}{" "}
              tokens
            </pre>
          </Card>
          <Card title="the questions asked" note="every one is a noul, a choice or a score">
            <div className="scroll-box">
              <pre>{JSON.stringify(data.asks, null, 2)}</pre>
            </div>
          </Card>
          <Card title="the verdicts returned" note="this is all jev produced">
            <div className="scroll-box">
              <pre>{JSON.stringify(data.verdicts, null, 2)}</pre>
            </div>
          </Card>
          <Card title="the typed output" note="composed deterministically from those verdicts">
            <div className="scroll-box">
              <pre>{JSON.stringify(data.output, null, 2)}</pre>
            </div>
          </Card>
          <Card title="the state that was judged" note="what a live model would see, exactly">
            <div className="scroll-box">
              <pre>{JSON.stringify(data.state, null, 2)}</pre>
            </div>
          </Card>
        </div>
      )}
    </Drawer>
  );
}
