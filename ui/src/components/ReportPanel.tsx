/**
 * The Markdown report, byte for byte what the CLI writes to `out/<domain>/report.md`.
 *
 * Shown as the Markdown it is rather than rendered: the point is that the
 * browser and the terminal are reading the same artefact, not a second one
 * prepared for the web.
 */

import { useState } from "react";
import { getReport } from "../api/client";
import type { Domain } from "../api/types";
import { useFetch } from "../hooks/useApi";
import { Banner, Card, Empty, Tabs } from "./ui";

export function ReportPanel({ runId, domains }: { runId: string; domains: Domain[] }) {
  const [domain, setDomain] = useState<Domain>(domains[0] ?? "fx");
  const { data, error, loading } = useFetch(
    () => getReport(runId, domain),
    `${runId}/report/${domain}`,
  );

  return (
    <div className="stack">
      {domains.length > 1 && (
        <Tabs
          tabs={domains.map((id) => ({ id, label: id }))}
          active={domain}
          onChange={setDomain}
        />
      )}
      <Card title={`${domain} report.md`} note="the same file the cli writes to disk">
        {loading && <Empty>reading the report…</Empty>}
        {error && <Banner kind="bad">{error}</Banner>}
        {data && (
          <div className="scroll-box" style={{ maxHeight: "70vh" }}>
            <pre>{data}</pre>
          </div>
        )}
      </Card>
    </div>
  );
}
