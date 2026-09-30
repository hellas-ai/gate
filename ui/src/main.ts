import "./style.css";
import { api } from "./api.ts";
import type {
  AppStatus,
  ExecutionEvent,
  GatewayAccess,
  HistoryEntry,
  ProviderConfig,
  RunKind,
  RunRequest,
} from "./types.ts";

type View = "run" | "serve" | "gateway" | "history" | "settings";

const rootElement = document.querySelector<HTMLDivElement>("#app");
if (!rootElement) throw new Error("missing #app");
const root = rootElement;

let view: View = "run";
let status: AppStatus | null = null;
let history: HistoryEntry[] = [];
let output = "";
let runState = "Ready";
let gatewayAccess: GatewayAccess | null = null;

root.innerHTML = `
  <div class="shell">
    <aside>
      <div class="brand"><span class="mark">Η</span><div>Hellas<small>Gate</small></div></div>
      <nav>
        ${navButton("run", "Run")}
        ${navButton("serve", "Serve")}
        ${navButton("gateway", "Gateway")}
        ${navButton("history", "History")}
        ${navButton("settings", "Settings")}
      </nav>
      <div id="rail-status" class="rail-status">Connecting…</div>
    </aside>
    <main id="content"></main>
  </div>`;

root.querySelectorAll<HTMLButtonElement>("[data-view]").forEach((button) => {
  button.addEventListener("click", () => {
    view = button.dataset.view as View;
    render();
  });
});

function navButton(id: View, label: string): string {
  return `<button data-view="${id}">${label}</button>`;
}

function render(): void {
  root.querySelectorAll("[data-view]").forEach((element) => {
    element.classList.toggle("active", (element as HTMLElement).dataset.view === view);
  });
  const content = document.querySelector<HTMLElement>("#content");
  if (!content) return;
  if (view === "run") renderRun(content);
  if (view === "serve") renderService(content, "provider");
  if (view === "gateway") renderService(content, "gateway");
  if (view === "history") renderHistory(content);
  if (view === "settings") renderSettings(content);
}

function renderRun(content: HTMLElement): void {
  content.innerHTML = `
    <header><p class="eyebrow">Verified execution</p><h1>Run on Hellas</h1>
      <p>Choose paid or authorized work and select its provider trust policy. Gate keeps the complete transcript locally.</p></header>
    <section class="panel composer">
      <div class="segmented">
        <button class="selected" data-kind="fetch">Sealed Fetch</button>
      </div>
      ${offerForm("")}
      <label>Request JSON<textarea id="input" rows="8" placeholder='{"model":"gpt-5","input":"Hello","stream":true}'></textarea></label>
      <div class="actions"><span id="run-state" class="muted">${escapeHtml(runState)}</span><button id="run" class="primary">Run</button></div>
    </section>
    <section class="panel output"><div class="panel-title">Output</div><pre id="output">${escapeHtml(output || "No execution yet.")}</pre></section>`;

  bindFundingForm(content, "");
  let kind: RunKind = "fetch";
  content.querySelectorAll<HTMLButtonElement>("[data-kind]").forEach((button) => {
    button.addEventListener("click", () => {
      kind = button.dataset.kind as RunKind;
      content.querySelectorAll("[data-kind]").forEach((item) => item.classList.remove("selected"));
      button.classList.add("selected");
    });
  });
  content.querySelector<HTMLButtonElement>("#run")?.addEventListener("click", async () => {
    const input = value("#input");
    output = "";
    setRunState("Starting…");
    try {
      await api.run({
        ...offerConfig(""), kind, input,
      }, receiveEvent);
    } catch (error) {
      setRunState(errorMessage(error));
    }
  });
}

function receiveEvent(event: ExecutionEvent): void {
  if (event.type === "started") setRunState(`Running ${event.data.runId}`);
  if (event.type === "output") output += event.data.text;
  if (event.type === "verification") output += `\n\nVerification: ${event.data.summary}`;
  if (event.type === "finished") setRunState("Verified and complete");
  if (event.type === "failed") setRunState(event.data.message);
  const element = document.querySelector("#output");
  if (element) element.textContent = output || "No output.";
}

function renderService(content: HTMLElement, service: "provider" | "gateway"): void {
  const current = status?.[service];
  const title = service === "provider" ? "Serve sealed Fetch" : "Loopback gateway";
  const description = service === "provider"
    ? "Serve paid and authorized Fetch through your attested Hellas identity."
    : "Expose an authenticated OpenAI-compatible endpoint only on this machine.";
  content.innerHTML = `
    <header><p class="eyebrow">${service}</p><h1>${title}</h1><p>${description}</p></header>
    <section class="panel service-card">
      <div><div class="panel-title">Runtime</div><h2 id="service-state">${escapeHtml(current?.state ?? "stopped")}</h2>
      <p id="service-detail">${escapeHtml(current?.detail ?? "Loading…")}</p></div>
      <button id="toggle" class="primary">${current?.state === "running" ? "Stop" : "Start"}</button>
    </section>
    ${service === "provider" ? providerForm() : gatewayForm()}`;
  if (service === "gateway") bindFundingForm(content, "gateway-");
  for (const [id, preview] of [["preview-bond", true], ["provision-paid-offer", false]] as const) {
    content.querySelector(`#${id}`)?.addEventListener("click", async () => {
      try {
        const result = await api.provisionPaidOffer(value("#paid-offer-config"), preview);
        const output = content.querySelector<HTMLTextAreaElement>("#paid-offer-result");
        if (output) output.value = result;
      } catch (error) { window.alert(errorMessage(error)); }
    });
  }
  content.querySelector<HTMLButtonElement>("#export-offers")?.addEventListener("click", async () => {
    try {
      const offers = await api.exportOffers();
      const target = content.querySelector("#offers");
      if (target) target.innerHTML = `<p class="muted">Copy each Offer to its contact. Import within five minutes; export again to renew.</p>` + offers.map((item) => `<label>Contact ${escapeHtml(item.contact)}<textarea rows="3" readonly>${escapeHtml(item.offer)}</textarea></label>`).join("");
    } catch (error) { window.alert(errorMessage(error)); }
  });
  content.querySelector<HTMLButtonElement>("#toggle")?.addEventListener("click", async () => {
    try {
      const running = status?.[service].state === "running";
      status = service === "provider"
        ? await api.setProvider(
            !running,
            running ? undefined : providerConfig(),
          )
        : await api.setGateway(
            !running,
            running ? undefined : gatewayConfig(),
          );
      gatewayAccess = status.gateway.state === "running" ? await api.gatewayAccess() : null;
      render();
      updateRail();
    } catch (error) {
      window.alert(errorMessage(error));
    }
  });
}

function providerForm(): string {
  return `<section class="panel"><div class="panel-title">Provider configuration</div>
    <div class="field-row"><label>Backend<input value="OpenAI Responses" readonly /></label>
    <label>OpenAI API key<input id="provider-api-key" type="password" autocomplete="off" /></label></div>
    <div class="field-row"><label>Fetch service<input id="provider-service" value="openai" /></label>
    <label>Fetch method<input id="provider-method" value="responses" /></label></div>
    <label>Paid Work config file<input id="provider-work-config" placeholder="/absolute/path/provider-work.json" /></label>
    <label>HTTPS routes JSON<textarea id="provider-https" rows="4" placeholder="Optional array of routes, accounts and resource policies"></textarea></label>
    <label>Imported contacts<textarea id="provider-contacts" rows="4" placeholder="one base64 contact enrollment per line">${escapeHtml(localStorage.getItem("gate.contacts") ?? "")}</textarea></label>
    <label>Requests per contact per day<input id="provider-quota" type="number" min="0" value="${escapeHtml(localStorage.getItem("gate.quota") ?? "100")}" /></label>
    <p class="muted">Each contact can use the configured accounts within this daily request allowance. Jobs have a five-minute deadline. To remove a contact, stop the provider and restart with the revised list. Removed grants remain revoked.</p>
    <label>Paid offer config file<input id="paid-offer-config" placeholder="/absolute/path/offer.json" /></label>
    <div class="actions"><button id="preview-bond">Preview bond</button><button id="provision-paid-offer">Provision paid offer</button></div>
    <textarea id="paid-offer-result" rows="5" readonly placeholder="Copy the provisioned fields into the client's pool configuration"></textarea>
    <button id="export-offers">Export Offers for enabled contacts</button><div id="offers"></div>
    <div class="field-row"><label>Port<input id="provider-port" type="number" min="1" max="65535" placeholder="automatic" /></label></div>
    <p class="muted">The API key crosses the typed IPC boundary once and remains only in native memory for this provider run.</p></section>`;
}

function providerConfig(): ProviderConfig {
  const rawPort = value("#provider-port");
  localStorage.setItem("gate.contacts", value("#provider-contacts"));
  localStorage.setItem("gate.quota", value("#provider-quota"));
  return {
    service: value("#provider-service"),
    method: value("#provider-method"),
    openaiApiKey: value("#provider-api-key"),
    workConfigPath: value("#provider-work-config") || undefined,
    httpsConfig: value("#provider-https") || undefined,
    contacts: value("#provider-contacts").split(/\n/).map((item) => item.trim()).filter(Boolean),
    requestsPerDay: Number(value("#provider-quota")),
    port: rawPort ? Number(rawPort) : undefined,
  };
}

function gatewayForm(): string {
  return `<section class="panel"><div class="panel-title">Local access</div>
    ${offerForm("gateway-")}
    <div class="field-row"><label>Bind address<input value="127.0.0.1 (ephemeral port)" readonly /></label>
    <label>Backend<input value="Verified sealed Fetch Responses" readonly /></label></div>
    <p class="muted">A fresh bearer is generated for each running instance.</p>
    ${gatewayAccess ? `<div class="definition"><div><span>Base URL</span><code>${escapeHtml(gatewayAccess.address)}</code></div><div><span>Bearer</span><code>${escapeHtml(gatewayAccess.bearer)}</code></div></div>` : ""}</section>`;
}

function gatewayConfig(): RunRequest {
  return { ...offerConfig("gateway-"), kind: "fetch", input: "{}" };
}

function offerForm(prefix: string): string {
  const appId = localStorage.getItem("gate.apple-app-id") ?? status?.identity.appleAppId ?? "";
  const hashes = localStorage.getItem("gate.apple-cdhashes") ?? status?.identity.appleCdHashes.join(", ") ?? "";
  const assurance = localStorage.getItem("gate.assurance") ?? "appleAppAttest";
  const funding = localStorage.getItem("gate.funding") ?? "authorized";
  return `<label>Funding<select id="${prefix}funding"><option value="authorized" ${funding === "authorized" ? "selected" : ""}>Authorized</option><option value="paid" ${funding === "paid" ? "selected" : ""}>Paid</option></select></label>
    <div id="${prefix}authorized-fields"><label>Resource name<input id="${prefix}resource" value="${escapeHtml(localStorage.getItem("gate.resource") ?? "responses")}" /></label>
    <label>Imported Offer<textarea id="${prefix}offer" rows="3" placeholder="paste the base64 Offer exported for this contact">${escapeHtml(localStorage.getItem("gate.offer") ?? "")}</textarea></label>
    </div><div id="${prefix}paid-fields"><label>Paid pool config file<input id="${prefix}pool-config" value="${escapeHtml(localStorage.getItem("gate.pool-config") ?? "")}" placeholder="/absolute/path/pool.json" /></label>
    <label>Provider endpoint ID<input id="${prefix}paid-provider" value="${escapeHtml(localStorage.getItem("gate.paid-provider") ?? "")}" /></label>
    <div class="field-row"><label>Fetch service<input id="${prefix}paid-service" value="${escapeHtml(localStorage.getItem("gate.paid-service") ?? "")}" placeholder="For an open HTTPS policy" /></label>
    <label>Fetch method<input id="${prefix}paid-method" value="${escapeHtml(localStorage.getItem("gate.paid-method") ?? "")}" placeholder="For an open HTTPS policy" /></label></div>
    <p class="muted">The pool file contains your payment coins and the provider's signed offer. Apple app ID and CDHashes for paid targets come from that file.</p></div>
    <label>Assurance<select id="${prefix}assurance"><option value="appleAppAttest" ${assurance === "appleAppAttest" ? "selected" : ""}>Apple App Attest</option><option value="producerSigned" ${assurance === "producerSigned" ? "selected" : ""}>Producer signed</option></select></label>
    <div id="${prefix}authorized-trust" class="field-row"><label>Trusted Apple app ID<input id="${prefix}apple-app-id" value="${escapeHtml(appId)}" placeholder="TEAMID.ai.hellas.gate" /></label>
    <label>Trusted CDHashes<input id="${prefix}apple-cdhashes" value="${escapeHtml(hashes)}" placeholder="from trusted release metadata, comma separated" /></label></div>`;
}

function offerConfig(prefix: string): Omit<RunRequest, "kind" | "input"> {
  for (const field of ["funding", "resource", "pool-config", "paid-provider", "paid-service", "paid-method", "offer", "assurance", "apple-app-id", "apple-cdhashes"]) {
    localStorage.setItem(`gate.${field}`, value(`#${prefix}${field}`));
  }
  return {
    target: value(`#${prefix}funding`) === "paid"
      ? { funding: "paid", poolConfig: value(`#${prefix}pool-config`), provider: value(`#${prefix}paid-provider`),
          route: value(`#${prefix}paid-service`) || value(`#${prefix}paid-method`)
            ? { service: value(`#${prefix}paid-service`), method: value(`#${prefix}paid-method`) } : undefined }
      : { funding: "authorized", offer: value(`#${prefix}offer`), resource: value(`#${prefix}resource`) },
    assurance: value(`#${prefix}assurance`) === "appleAppAttest" ? "appleAppAttest" : "producerSigned",
    appleAppId: value(`#${prefix}apple-app-id`),
    appleCdHashes: value(`#${prefix}apple-cdhashes`).split(",").map((item) => item.trim()).filter(Boolean),
  };
}

function bindFundingForm(content: HTMLElement, prefix: string): void {
  const select = content.querySelector<HTMLSelectElement>(`#${prefix}funding`);
  const refresh = (): void => {
    const paid = select?.value === "paid";
    for (const field of ["authorized-fields", "authorized-trust", "paid-fields"]) {
      const element = content.querySelector<HTMLElement>(`#${prefix}${field}`);
      if (element) element.hidden = field === "paid-fields" ? !paid : paid;
    }
  };
  select?.addEventListener("change", refresh);
  refresh();
}

function renderHistory(content: HTMLElement): void {
  content.innerHTML = `<header><p class="eyebrow">Private local data</p><h1>History</h1>
    <p>Requests, results, verification, and failures stay in Gate unless you explicitly export them.</p></header>
    <div class="toolbar"><button id="refresh">Refresh</button><button id="clear" class="danger">Clear all</button></div>
    <section class="history-list">${history.map(historyCard).join("") || '<div class="panel muted">No history.</div>'}</section>`;
  content.querySelector("#refresh")?.addEventListener("click", loadHistory);
  content.querySelector("#clear")?.addEventListener("click", async () => {
    if (!window.confirm("Permanently clear all local Gate history?")) return;
    await api.clearHistory();
    await loadHistory();
  });
  content.querySelectorAll<HTMLButtonElement>("[data-delete]").forEach((button) => {
    button.addEventListener("click", async () => {
      await api.deleteHistory(button.dataset.delete ?? "");
      await loadHistory();
    });
  });
}

function historyCard(entry: HistoryEntry): string {
  return `<article class="panel history-card"><div><span class="pill ${escapeHtml(entry.status)}">${escapeHtml(entry.status)}</span>
    <h3>${escapeHtml(entry.kind)} · ${escapeHtml(entry.target)}</h3><time>${new Date(entry.createdAtMs).toLocaleString()}</time></div>
    <p>${escapeHtml(entry.request)}</p>${entry.error ? `<p class="error">${escapeHtml(entry.error)}</p>` : ""}
    <button data-delete="${escapeHtml(entry.id)}">Delete</button></article>`;
}

function renderSettings(content: HTMLElement): void {
  const identity = status?.identity;
  content.innerHTML = `<header><p class="eyebrow">Host</p><h1>Settings & diagnostics</h1></header>
    <section class="panel definition"><div><span>App Attest</span><strong>${escapeHtml(identity?.attestation ?? "loading")}</strong></div>
    <p>${escapeHtml(identity?.detail ?? "")}</p><div><span>Local control socket</span><code>${escapeHtml(status?.socketPath ?? "")}</code></div>
    <div><span>Contact ID</span><code>${escapeHtml(identity?.contactId ?? "")}</code></div>
    <label>Export contact enrollment<textarea rows="4" readonly>${escapeHtml(identity?.contact ?? "")}</textarea></label>
    <p>Copy this public contact enrollment to a provider to request a grant.</p>
    <div><span>Version</span><strong>${escapeHtml(status?.version ?? "")}</strong></div></section>`;
}

async function refreshStatus(): Promise<void> {
  try {
    status = await api.status();
    if (status.gateway.state === "running" && !gatewayAccess) {
      gatewayAccess = await api.gatewayAccess();
    } else if (status.gateway.state !== "running") {
      gatewayAccess = null;
    }
    updateRail();
    refreshVisibleStatus();
  } catch (error) {
    const rail = document.querySelector("#rail-status");
    if (rail) rail.textContent = errorMessage(error);
  }
}

function refreshVisibleStatus(): void {
  if (!status) return;
  if (view === "settings") {
    render();
    return;
  }
  const service = view === "serve" ? "provider" : view === "gateway" ? "gateway" : null;
  if (!service) return;
  const current = status[service];
  const state = document.querySelector("#service-state");
  const detail = document.querySelector("#service-detail");
  const toggle = document.querySelector<HTMLButtonElement>("#toggle");
  if (state) state.textContent = current.state;
  if (detail) detail.textContent = current.detail;
  if (toggle) toggle.textContent = current.state === "running" ? "Stop" : "Start";
}

async function loadHistory(): Promise<void> {
  history = await api.history();
  if (view === "history") render();
}

function updateRail(): void {
  const rail = document.querySelector("#rail-status");
  if (!rail || !status) return;
  rail.innerHTML = `<i class="dot ${status.provider.state}"></i> Provider ${escapeHtml(status.provider.state)}<br>
    <i class="dot ${status.gateway.state}"></i> Gateway ${escapeHtml(status.gateway.state)}`;
}

function value(selector: string): string {
  return document.querySelector<HTMLInputElement | HTMLTextAreaElement>(selector)?.value.trim() ?? "";
}

function setRunState(message: string): void {
  runState = message;
  const element = document.querySelector("#run-state");
  if (element) element.textContent = message;
}

function errorMessage(error: unknown): string {
  return typeof error === "object" && error !== null && "message" in error
    ? String((error as { message: unknown }).message)
    : String(error);
}

function escapeHtml(value: string): string {
  return value.replace(/[&<>'"]/g, (character) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", "'": "&#39;", '"': "&quot;",
  })[character] ?? character);
}

render();
void refreshStatus();
void loadHistory();
window.setInterval(refreshStatus, 5_000);
