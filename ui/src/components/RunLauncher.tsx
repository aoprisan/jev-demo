/** Starting a run: the CLI's flags, as a form. */

import { useState } from "react";
import { startRun } from "../api/client";
import { errorMessage } from "../hooks/useApi";
import type { Domain, RunView, ServerInfo } from "../api/types";

export function RunLauncher({
  info,
  onStarted,
}: {
  info: ServerInfo | null;
  onStarted: (run: RunView) => void;
}) {
  const [domain, setDomain] = useState<Domain>("fx");
  const [seed, setSeed] = useState("20250106");
  const [days, setDays] = useState("90");
  const [limit, setLimit] = useState("");
  const [mock, setMock] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const run = await startRun({
        domain,
        seed: Number(seed),
        days: Number(days),
        limit: limit.trim() === "" ? null : Number(limit),
        mock,
      });
      onStarted(run);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit}>
      <div className="card-title">
        <h3>new run</h3>
      </div>

      <div className="field">
        <label htmlFor="domain">desk</label>
        <select
          id="domain"
          value={domain}
          onChange={(event) => setDomain(event.target.value as Domain)}
        >
          <option value="fx">forex — mean reversion, 4 stages</option>
          <option value="battery">battery — DP arbitrage, 5 stages</option>
          <option value="all">both, then one Explain for compliance</option>
        </select>
      </div>

      <div className="row">
        <div className="field">
          <label htmlFor="seed">seed</label>
          <input
            id="seed"
            type="number"
            value={seed}
            min={0}
            onChange={(event) => setSeed(event.target.value)}
            required
          />
        </div>
        <div className="field">
          <label htmlFor="days">days</label>
          <input
            id="days"
            type="number"
            value={days}
            min={1}
            max={info?.max_days ?? 365}
            onChange={(event) => setDays(event.target.value)}
            required
          />
        </div>
      </div>

      <div className="field">
        <label htmlFor="limit">judge only the first N days (optional)</label>
        <input
          id="limit"
          type="number"
          min={1}
          value={limit}
          placeholder="all of them"
          onChange={(event) => setLimit(event.target.value)}
        />
      </div>

      <label className="check">
        <input
          type="checkbox"
          checked={mock}
          onChange={(event) => setMock(event.target.checked)}
        />
        offline rule-based backend
      </label>

      {!mock && !info?.live_available && (
        <p className="dim" style={{ fontSize: 11.5, marginTop: -4 }}>
          the server has no <code>TYPESAFE_API_KEY</code>, so a live run will be refused.
        </p>
      )}

      <button className="btn" type="submit" disabled={busy}>
        {busy ? <span className="spin" /> : "▸"} run
      </button>

      {error && (
        <p className="bad mono" style={{ fontSize: 11.5, marginBottom: 0 }}>
          {error}
        </p>
      )}
    </form>
  );
}
