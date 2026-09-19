/**
 * The typed client.
 *
 * One fetch wrapper, one error type, and a function per endpoint. Every
 * response is typed by `types.ts`, which mirrors the server's `dto.rs`.
 */

import type {
  BatteryDayDetail,
  CallPage,
  CallRecord,
  CallRequest,
  Domain,
  FxDecisionDetail,
  PromptView,
  RunRequest,
  RunSummary,
  RunView,
  ServerInfo,
} from "./types";

/** A failure the API reported, carrying the status and its own message. */
export class ApiError extends Error {
  readonly status: number;
  readonly kind: string;

  constructor(status: number, kind: string, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.kind = kind;
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  let response: Response;
  try {
    response = await fetch(`/api${path}`, {
      ...init,
      headers: { "content-type": "application/json", ...(init?.headers ?? {}) },
    });
  } catch (cause) {
    // A dead server and a refused connection read the same to a user: the
    // API is not there. Saying so beats a bare "Failed to fetch".
    throw new ApiError(0, "unreachable", `cannot reach the API: ${String(cause)}`);
  }

  if (!response.ok) {
    const body = (await response.json().catch(() => null)) as
      | { error?: string; message?: string }
      | null;
    throw new ApiError(
      response.status,
      body?.error ?? "http_error",
      body?.message ?? `${response.status} ${response.statusText}`,
    );
  }

  if (response.status === 204) return undefined as T;
  return (await response.json()) as T;
}

async function requestText(path: string): Promise<string> {
  const response = await fetch(`/api${path}`);
  if (!response.ok) {
    throw new ApiError(response.status, "http_error", `${response.status} ${response.statusText}`);
  }
  return await response.text();
}

/** What the server can do, and how it is configured. */
export const getInfo = (): Promise<ServerInfo> => request<ServerInfo>("/info");

/** Every run this process is holding, newest first. */
export const listRuns = (): Promise<RunSummary[]> => request<RunSummary[]>("/runs");

/** Start a run. Answers before it has finished judging. */
export const startRun = (body: RunRequest): Promise<RunView> =>
  request<RunView>("/runs", { method: "POST", body: JSON.stringify(body) });

/** One run, with its result once it is done. */
export const getRun = (id: string): Promise<RunView> => request<RunView>(`/runs/${id}`);

/** Forget a run. */
export const deleteRun = (id: string): Promise<void> =>
  request<void>(`/runs/${id}`, { method: "DELETE" });

/** One decision in full, including the features the judgment layer read. */
export const getFxDecision = (id: string, index: number): Promise<FxDecisionDetail> =>
  request<FxDecisionDetail>(`/runs/${id}/fx/decisions/${index}`);

/** One day in full: all three schedules, every stage, both executions. */
export const getBatteryDay = (id: string, day: number): Promise<BatteryDayDetail> =>
  request<BatteryDayDetail>(`/runs/${id}/battery/days/${day}`);

/** A page of the audit log. */
export const getCalls = (
  id: string,
  offset: number,
  limit: number,
  decision: string | null = null,
): Promise<CallPage> => {
  const query = new URLSearchParams({ offset: String(offset), limit: String(limit) });
  // Narrowed to one decision, the page walks that decision's calls only.
  if (decision !== null) query.set("decision", decision);
  return request<CallPage>(`/runs/${id}/calls?${query}`);
};

/** One call in full: the state judged, the questions, the verdicts, the output. */
export const getCall = (id: string, index: number): Promise<CallRecord> =>
  request<CallRecord>(`/runs/${id}/calls/${index}`);

/** The `POST /v1/systemone` body for one call, byte for byte what the live client sends. */
export const getCallRequest = (id: string, index: number): Promise<CallRequest> =>
  request<CallRequest>(`/runs/${id}/calls/${index}/request`);

/** One desk's `report.md`, byte for byte what the CLI writes to disk. */
export const getReport = (id: string, domain: Domain): Promise<string> =>
  requestText(`/runs/${id}/report?domain=${domain}`);

/** Each primitive's standing instructions. */
export const getPrompts = (): Promise<PromptView[]> => request<PromptView[]>("/prompts");

/** The JSON Schema of each primitive output. */
export const getSchemas = (): Promise<Record<string, unknown>> =>
  request<Record<string, unknown>>("/schemas");

/** Where a run's audit log can be downloaded from. */
export const decisionsUrl = (id: string): string => `/api/runs/${id}/decisions.jsonl`;
