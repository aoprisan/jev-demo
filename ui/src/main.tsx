import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./styles.css";

/**
 * Evict whatever another app left on this origin.
 *
 * jev-desk is served on a developer's localhost port, and it shares that
 * origin with every other project that was ever served there. A service worker
 * one of them registered outlives it and keeps answering fetches from its
 * cache — a stale run list, a poll frozen at "judging", or another app's shell
 * entirely when the server is down. jev-desk registers no worker of its own,
 * so any registration found here is foreign; unregister it, drop its caches,
 * and reload once so the page is fetched with nothing in between.
 */
async function evictForeignWorkers(): Promise<boolean> {
  if (!("serviceWorker" in navigator)) return false;
  try {
    const registrations = await navigator.serviceWorker.getRegistrations();
    if (registrations.length === 0) return false;
    await Promise.all(registrations.map((registration) => registration.unregister()));
    if ("caches" in window) {
      const names = await caches.keys();
      await Promise.all(names.map((name) => caches.delete(name)));
    }
    return true;
  } catch {
    // Storage may be blocked in a private window; the page still works, the
    // API answers with `no-store`, so a worker can at most slow it down.
    return false;
  }
}

const RELOADED = "jev-desk:evicted-workers";

/** True the first time only; a worker that refuses to go must not reload forever. */
function firstReload(): boolean {
  try {
    if (sessionStorage.getItem(RELOADED)) return false;
    sessionStorage.setItem(RELOADED, "1");
    return true;
  } catch {
    return false;
  }
}

async function boot() {
  const evicted = await evictForeignWorkers();
  // One reload is enough for the browser to fetch the page without the worker.
  if (evicted && firstReload()) {
    window.location.reload();
    return;
  }

  const root = document.getElementById("root");
  if (!root) throw new Error("index.html is missing its #root element");

  createRoot(root).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

void boot();
