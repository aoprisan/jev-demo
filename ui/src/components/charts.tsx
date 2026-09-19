/**
 * Hand-drawn SVG charts.
 *
 * Small enough to keep in the repository rather than take a charting
 * dependency for, and they inherit the palette rather than fighting it.
 */

import { num } from "../format";

export interface Series {
  name: string;
  values: number[];
  color: string;
  dashed?: boolean;
}

/**
 * Two or more cumulative curves on one pair of axes, with a zero line.
 *
 * The y-axis is shared, which is the point: the gated and ungated books are
 * only comparable drawn against the same scale.
 */
export function Curve({
  series,
  height = 160,
  xLabel,
  yFormat = (v: number) => num(v),
}: {
  series: Series[];
  height?: number;
  xLabel?: string;
  yFormat?: (value: number) => string;
}) {
  const width = 640;
  const pad = { top: 10, right: 8, bottom: 18, left: 56 };
  const length = Math.max(...series.map((s) => s.values.length), 1);
  const all = series.flatMap((s) => s.values);
  const rawMin = Math.min(0, ...all);
  const rawMax = Math.max(0, ...all);
  const span = rawMax - rawMin || 1;
  const min = rawMin - span * 0.05;
  const max = rawMax + span * 0.05;

  const x = (i: number) =>
    pad.left + (length <= 1 ? 0 : (i / (length - 1)) * (width - pad.left - pad.right));
  const y = (v: number) =>
    pad.top + (1 - (v - min) / (max - min)) * (height - pad.top - pad.bottom);

  const path = (values: number[]) => values.map((v, i) => `${x(i)},${y(v)}`).join(" ");

  return (
    <div>
      <svg
        className="chart"
        viewBox={`0 0 ${width} ${height}`}
        preserveAspectRatio="none"
        role="img"
        aria-label={series.map((s) => s.name).join(" against ")}
      >
        <line
          x1={pad.left}
          x2={width - pad.right}
          y1={y(0)}
          y2={y(0)}
          stroke="#2b333d"
          strokeWidth="1"
        />
        {[max, min].map((value, i) => (
          <text
            key={i}
            x={pad.left - 6}
            y={y(value) + (i === 0 ? 8 : -2)}
            textAnchor="end"
            fill="#6b7482"
            fontSize="10"
            fontFamily="ui-monospace, monospace"
          >
            {yFormat(value)}
          </text>
        ))}
        {series.map((s) => (
          <polyline
            key={s.name}
            points={path(s.values)}
            fill="none"
            stroke={s.color}
            strokeWidth="1.5"
            strokeDasharray={s.dashed ? "3 3" : undefined}
            vectorEffect="non-scaling-stroke"
          />
        ))}
      </svg>
      <div className="legend">
        {series.map((s) => (
          <span key={s.name}>
            <span className="swatch" style={{ background: s.color }} />
            {s.name}
          </span>
        ))}
        {xLabel && <span className="dim" style={{ marginLeft: "auto" }}>{xLabel}</span>}
      </div>
    </div>
  );
}

/**
 * A day's schedule: signed power per hour above and below zero, with the state
 * of charge drawn over it.
 *
 * Charging and discharging are the same axis with opposite signs, so one glance
 * says what the plan does and when.
 */
export function SchedulePlot({
  powerMw,
  socMwh,
  window: reserveWindow,
  height = 130,
}: {
  powerMw: number[];
  socMwh: number[];
  window?: { from: number; to: number } | null;
  height?: number;
}) {
  const width = 640;
  const pad = { top: 8, right: 8, bottom: 16, left: 34 };
  const hours = Math.max(powerMw.length, 1);
  const peak = Math.max(0.1, ...powerMw.map(Math.abs));
  const socMax = Math.max(0.1, ...socMwh);
  const plotW = width - pad.left - pad.right;
  const plotH = height - pad.top - pad.bottom;
  const slot = plotW / hours;
  const zero = pad.top + plotH / 2;
  const barH = (mw: number) => (Math.abs(mw) / peak) * (plotH / 2);
  const socY = (mwh: number) => pad.top + plotH - (mwh / socMax) * plotH;

  return (
    <div>
      <svg
        className="chart"
        viewBox={`0 0 ${width} ${height}`}
        preserveAspectRatio="none"
        role="img"
        aria-label="hourly power and state of charge"
      >
        {reserveWindow && (
          <rect
            x={pad.left + reserveWindow.from * slot}
            y={pad.top}
            width={(reserveWindow.to - reserveWindow.from + 1) * slot}
            height={plotH}
            fill="#e3b34112"
          />
        )}
        <line x1={pad.left} x2={width - pad.right} y1={zero} y2={zero} stroke="#2b333d" />
        {powerMw.map((mw, hour) => (
          <rect
            key={hour}
            x={pad.left + hour * slot + slot * 0.15}
            y={mw >= 0 ? zero - barH(mw) : zero}
            width={slot * 0.7}
            height={Math.max(0.5, barH(mw))}
            fill={mw >= 0 ? "#79b8ff" : "#6fd6bb"}
          />
        ))}
        <polyline
          points={socMwh.map((mwh, i) => `${pad.left + i * slot},${socY(mwh)}`).join(" ")}
          fill="none"
          stroke="#e3b341"
          strokeWidth="1.2"
          strokeDasharray="4 2"
          vectorEffect="non-scaling-stroke"
        />
        {[0, 6, 12, 18, 23].map((hour) => (
          <text
            key={hour}
            x={pad.left + hour * slot + slot / 2}
            y={height - 4}
            textAnchor="middle"
            fill="#6b7482"
            fontSize="9"
            fontFamily="ui-monospace, monospace"
          >
            {String(hour).padStart(2, "0")}
          </text>
        ))}
      </svg>
      <div className="legend">
        <span>
          <span className="swatch" style={{ background: "#79b8ff" }} />
          charge
        </span>
        <span>
          <span className="swatch" style={{ background: "#6fd6bb" }} />
          discharge
        </span>
        <span>
          <span className="swatch" style={{ background: "#e3b341" }} />
          state of charge
        </span>
        {reserveWindow && <span className="dim">shaded: the notice window</span>}
      </div>
    </div>
  );
}
