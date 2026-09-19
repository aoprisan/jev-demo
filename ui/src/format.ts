/** Number and label formatting, kept in one place so a column reads the same everywhere. */

const nf = (digits: number) =>
  new Intl.NumberFormat("en-GB", { minimumFractionDigits: digits, maximumFractionDigits: digits });

/** A number with thousands separators and fixed decimals. */
export const num = (value: number, digits = 0): string => nf(digits).format(value);

/** The same, with an explicit sign, the way the terminal report prints P&L. */
export const signed = (value: number, digits = 0): string =>
  `${value >= 0 ? "+" : "−"}${nf(digits).format(Math.abs(value))}`;

/** A 0..=1 fraction as a percentage. */
export const pct = (value: number, digits = 0): string => `${nf(digits).format(value * 100)}%`;

/** Basis points, signed. */
export const bps = (value: number, digits = 1): string => `${signed(value, digits)} bp`;

/** A duration in milliseconds, as seconds once it is long enough to matter. */
export const ms = (value: number): string =>
  value >= 1000 ? `${nf(1).format(value / 1000)}s` : `${nf(0).format(value)}ms`;

/** A dollar figure, at a precision that suits its size. */
export const usd = (value: number): string =>
  value >= 1 ? `$${nf(2).format(value)}` : `$${nf(4).format(value)}`;

/** A wall-clock time, local to the reader. */
export const clock = (epochMs: number): string =>
  new Date(epochMs).toLocaleTimeString("en-GB", { hour12: false });

/** `reserve_heavy` reads as `reserve heavy`; the wire keeps the underscore. */
export const words = (label: string): string => label.replace(/_/g, " ");

/** Which way a number should be coloured, or neither. */
export const tone = (value: number): "good" | "bad" | "muted" =>
  value > 0 ? "good" : value < 0 ? "bad" : "muted";
