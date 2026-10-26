import "./style.css";
import { api } from "./api.ts";
import type { AppStatus } from "./types.ts";
const element = document.querySelector<HTMLDivElement>("#app");
if (!element) throw new Error("missing app");
const root = element;
let status: AppStatus | null = null;
const escapeHtml = (value: unknown): string => String(value).replace(/[&<>"']/g, c => ({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[c] ?? c));
function error(value: unknown): void {
  const message = value && typeof value === "object" && "message" in value ? value.message : value;
  const output = document.querySelector("#error");
  if (output) output.textContent = String(message);
}
async function render(): Promise<void> {
  try { status = await api.status(); } catch (e) { error(e); return; }
  const id = status.identity;
  root.innerHTML = `<div class="shell"><aside><div class="brand">Hellas Gate</div><nav><button id="settings">Settings</button><button id="history">History</button></nav></aside><main><p id="error" role="alert"></p><div id="content"><h1>Settings</h1>
    ${id.initialized ? `<p>Person: ${escapeHtml(id.fingerprint)}</p><p>${escapeHtml(id.detail)}</p><dl><dt>Principal</dt><dd>${escapeHtml(id.principal)}</dd><dt>Node key</dt><dd>${escapeHtml(id.nodeKey)}</dd><dt>Transport</dt><dd>${escapeHtml(id.transportId)}</dd><dt>Authority</dt><dd>${escapeHtml(id.authority)} / generation ${id.generation}</dd><dt>Local control</dt><dd>${escapeHtml(status.socketPath)}</dd></dl><label>Signed contact card<textarea readonly>${escapeHtml(id.contactCard)}</textarea></label>` : `<p>${escapeHtml(id.detail)}</p><form id="initialize"><label>Name<input name="name" maxlength="64" required></label><label><input name="software" type="checkbox" required>I choose a software passkey for development (UP only).</label><button>Create identity</button></form>`}
  </div></main></div>`;
  document.querySelector("#settings")?.addEventListener("click", () => { void render(); });
  document.querySelector("#history")?.addEventListener("click", () => { void showHistory().catch(error); });
  document.querySelector<HTMLFormElement>("#initialize")?.addEventListener("submit", e => {
    e.preventDefault(); const name = new FormData(e.currentTarget as HTMLFormElement).get("name");
    void api.initializeSoftware(String(name)).then(render).catch(error);
  });
}
async function showHistory(): Promise<void> {
  const entries = await api.history();
  const content = document.querySelector("#content");
  if (!content) return;
  content.innerHTML = `<h1>History</h1><button id="clear">Clear history</button>${entries.map(entry => `<article><strong>${escapeHtml(entry.status)}</strong><pre>${escapeHtml(entry.request)}</pre><pre>${escapeHtml(entry.result)}</pre><p>${escapeHtml(entry.verification || entry.error)}</p><button data-delete="${escapeHtml(entry.id)}">Delete</button></article>`).join("")}`;
  document.querySelector("#clear")?.addEventListener("click", () => { void api.clearHistory().then(showHistory).catch(error); });
  content.querySelectorAll<HTMLButtonElement>("[data-delete]").forEach(button => button.addEventListener("click", () => { void api.deleteHistory(button.dataset.delete ?? "").then(showHistory).catch(error); }));
}
void render();
