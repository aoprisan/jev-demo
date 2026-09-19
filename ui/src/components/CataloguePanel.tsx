/**
 * The contract itself: each primitive's standing instructions, and the JSON
 * Schema of each output.
 *
 * The prompts are identical across both domains — domain framing is injected as
 * a context block, never as a separate prompt — and the schemas are what
 * `validate()` enforces in code. Both are served straight from the binary.
 */

import { useState } from "react";
import { getPrompts, getSchemas } from "../api/client";
import { useFetch } from "../hooks/useApi";
import { Banner, Card, Empty, Tabs } from "./ui";

type View = "prompts" | "schemas";

export function CataloguePanel() {
  const [view, setView] = useState<View>("prompts");
  return (
    <div className="stack">
      <Tabs
        tabs={[
          { id: "prompts", label: "standing instructions" },
          { id: "schemas", label: "output schemas" },
        ]}
        active={view}
        onChange={setView}
      />
      {view === "prompts" ? <Prompts /> : <Schemas />}
    </div>
  );
}

function Prompts() {
  const { data, error, loading } = useFetch(getPrompts, "prompts");
  if (loading) return <Empty>reading the prompts…</Empty>;
  if (error) return <Banner kind="bad">{error}</Banner>;
  if (!data) return null;

  return (
    <div className="stack">
      {data.map((prompt) => (
        <Card key={prompt.primitive} title={prompt.primitive} note="identical across domains">
          <div className="scroll-box">
            <pre>{prompt.text}</pre>
          </div>
        </Card>
      ))}
    </div>
  );
}

function Schemas() {
  const { data, error, loading } = useFetch(getSchemas, "schemas");
  if (loading) return <Empty>reading the schemas…</Empty>;
  if (error) return <Banner kind="bad">{error}</Banner>;
  if (!data) return null;

  return (
    <div className="stack">
      <Banner kind="accent">
        A schema violation is an error, not a degraded answer: a value outside its range, a string
        past its bound, a label outside the enum offered, a ranking that does not cover its
        candidates, or a contradiction — a hold carrying size. Nothing falls back to execute.
      </Banner>
      {Object.entries(data).map(([name, schema]) => (
        <Card key={name} title={name}>
          <div className="scroll-box">
            <pre>{JSON.stringify(schema, null, 2)}</pre>
          </div>
        </Card>
      ))}
    </div>
  );
}
