/** The small, shared pieces: cards, stats, badges, tabs, tables and the drawer. */

import type { ReactNode } from "react";
import { useEffect } from "react";
import type { Action, CheckView } from "../api/types";
import { num, pct, words } from "../format";

export function Card({
  title,
  note,
  right,
  children,
}: {
  title?: string;
  note?: string;
  right?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="card">
      {(title || right) && (
        <header className="card-title">
          {title && <h3>{title}</h3>}
          {note && <span className="note">{note}</span>}
          <span style={{ flex: 1 }} />
          {right}
        </header>
      )}
      {children}
    </section>
  );
}

export function Stat({
  label,
  value,
  sub,
  tone,
}: {
  label: string;
  value: ReactNode;
  sub?: ReactNode;
  tone?: "good" | "bad" | "warn" | "muted" | "accent";
}) {
  return (
    <div className="stat">
      <div className="label">{label}</div>
      <div className={`value ${tone ?? ""}`}>{value}</div>
      {sub !== undefined && <div className="sub">{sub}</div>}
    </div>
  );
}

export function StatGrid({ children }: { children: ReactNode }) {
  return <div className="grid stats">{children}</div>;
}

/** A gate action, coloured by what it does. */
export function ActionBadge({ action, size }: { action: Action; size?: number }) {
  return (
    <span className={`badge ${action}`}>
      {action}
      {size !== undefined && action !== "hold" && ` ${pct(size)}`}
    </span>
  );
}

export function Badge({
  children,
  kind,
}: {
  children: ReactNode;
  kind?: "ok" | "fail" | "plain";
}) {
  return <span className={`badge ${kind ?? "plain"}`}>{children}</span>;
}

/** The named plausibility checks, each with the probability behind it. */
export function CheckList({ checks }: { checks: CheckView[] }) {
  return (
    <div className="checklist">
      {checks.map((check) => (
        <div className="item" key={check.name}>
          <Badge kind={check.ok ? "ok" : "fail"}>{check.ok ? "ok" : "fail"}</Badge>
          <div>
            <div className="mono">{check.name}</div>
            <div className="note">{check.note}</div>
          </div>
        </div>
      ))}
    </div>
  );
}

/** A labelled horizontal bar list: distributions, counts, rankings. */
export function BarList({
  rows,
  total,
  render,
}: {
  rows: { label: string; value: number }[];
  total?: number;
  render?: (value: number) => string;
}) {
  const max = total ?? Math.max(1, ...rows.map((r) => r.value));
  return (
    <div className="bars">
      {rows.map((row) => (
        <div className="bar-row" key={row.label}>
          <span>{words(row.label)}</span>
          <span className="track">
            <span className="fill" style={{ width: `${(row.value / max) * 100}%` }} />
          </span>
          <span className="count">{render ? render(row.value) : num(row.value)}</span>
        </div>
      ))}
    </div>
  );
}

/** A 0..=1 bar, for a confidence or a size factor. */
export function Meter({ value }: { value: number }) {
  return (
    <div className="meter">
      <span style={{ width: `${Math.max(0, Math.min(1, value)) * 100}%` }} />
    </div>
  );
}

export function Tabs<T extends string>({
  tabs,
  active,
  onChange,
}: {
  tabs: { id: T; label: string; badge?: ReactNode }[];
  active: T;
  onChange: (id: T) => void;
}) {
  return (
    <div className="tabs" role="tablist">
      {tabs.map((tab) => (
        <button
          key={tab.id}
          role="tab"
          className="tab"
          aria-selected={tab.id === active}
          onClick={() => onChange(tab.id)}
        >
          {tab.label}
          {tab.badge !== undefined && <span className="dim"> {tab.badge}</span>}
        </button>
      ))}
    </div>
  );
}

export function Empty({ children }: { children: ReactNode }) {
  return <p className="empty">{children}</p>;
}

export function Banner({
  children,
  kind,
}: {
  children: ReactNode;
  kind?: "warn" | "bad" | "accent";
}) {
  return <p className={`banner ${kind ?? ""}`}>{children}</p>;
}

/** A definition list, for a pane of labelled values. */
export function KeyValue({ rows }: { rows: [string, ReactNode][] }) {
  return (
    <dl className="kv">
      {rows.map(([key, value]) => (
        <div key={key} style={{ display: "contents" }}>
          <dt>{key}</dt>
          <dd>{value}</dd>
        </div>
      ))}
    </dl>
  );
}

/** A right-hand drawer. Closes on Escape and on a click outside it. */
export function Drawer({
  title,
  onClose,
  children,
}: {
  title: ReactNode;
  onClose: () => void;
  children: ReactNode;
}) {
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="drawer-backdrop" onClick={onClose} role="presentation">
      <aside
        className="drawer"
        role="dialog"
        aria-modal="true"
        onClick={(event) => event.stopPropagation()}
      >
        <header className="drawer-head">
          <h2 style={{ fontSize: 14 }}>{title}</h2>
          <button className="btn ghost" onClick={onClose}>
            close (esc)
          </button>
        </header>
        {children}
      </aside>
    </div>
  );
}
