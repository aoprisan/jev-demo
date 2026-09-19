/**
 * JSON, coloured.
 *
 * `JSON.stringify` and then one tokenising pass over the result: keys, strings,
 * numbers and the three literals each get a class, and everything else —
 * punctuation, indentation — is passed through as plain text and takes the
 * block's dim base colour. Dependency-free on purpose: the UI carries no
 * libraries, and highlighting one grammar we serialise ourselves is a regex,
 * not a package.
 */

import type { ReactNode } from "react";

/** A string (optionally the key half of a member), a literal, or a number. */
const TOKEN = /"(?:\\.|[^"\\])*"(\s*:)?|\b(?:true|false|null)\b|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/g;

/** A pretty-printed, syntax-highlighted block for any serialisable value. */
export function Json({ value }: { value: unknown }) {
  return <pre className="json">{highlight(JSON.stringify(value, null, 2) ?? "null")}</pre>;
}

function highlight(text: string): ReactNode[] {
  const parts: ReactNode[] = [];
  let last = 0;

  for (const m of text.matchAll(TOKEN)) {
    const [token, colon] = m;
    if (m.index > last) parts.push(text.slice(last, m.index));

    if (colon !== undefined) {
      // A member name: colour the quoted name, leave the colon to the punctuation.
      parts.push(span("key", token.slice(0, token.length - colon.length), m.index));
      parts.push(colon);
    } else {
      parts.push(span(cls(token), token, m.index));
    }
    last = m.index + token.length;
  }

  if (last < text.length) parts.push(text.slice(last));
  return parts;
}

function cls(token: string): string {
  if (token.startsWith('"')) return "str";
  return token === "true" || token === "false" || token === "null" ? "lit" : "num";
}

function span(kind: string, text: string, at: number) {
  return (
    <span key={at} className={`j-${kind}`}>
      {text}
    </span>
  );
}
