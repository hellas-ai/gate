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
      <p>Choose the peer and trust anchor explicitly. Gate keeps the complete transcript locally.</p></header>
    <section class="panel composer">
      <div class="segmented">
        <button class="selected" data-kind="fetch">Sealed Fetch</button>
      </div>
      <div class="field-row">
        <label>Target<input id="target" placeholder="provider endpoint or relay target" /></label>
        <label>Trust anchor<input id="trust" placeholder="provider genesis / trusted pin" /></label>
      </div>
      <div class="field-row">
        <label>Execution environment<input id="environment" placeholder="content ID" /></label>
        <label>Assurance<select id="assurance"><option value="producerSigned">Producer signed</option><option value="appleAppAttest">Apple App Attest</option></select></label>
      </div>
      <div class="field-row">
        <label>Fetch service<input id="service" value="openai" /></label>
        <label>Fetch method<input id="method" value="responses" /></label>
      </div>
      <div class="field-row">
        <label>Apple app ID<input id="apple-app-id" placeholder="TEAMID.ai.hellas.gate" /></label>
        <label>Allowed CDHashes<input id="apple-cdhashes" placeholder="hex, comma separated" /></label>
      </div>
      <label>OpenAI Responses request<textarea id="input" rows="8" placeholder='{"model":"gpt-5","input":"Hello","stream":true}'></textarea></label>
      <div class="actions"><span id="run-state" class="muted">${escapeHtml(runState)}</span><button id="run" class="primary">Run</button></div>
    </section>
    <section class="panel output"><div class="panel-title">Output</div><pre id="output">${escapeHtml(output || "No execution yet.")}</pre></section>`;

  let kind: RunKind = "fetch";
  content.querySelectorAll<HTMLButtonElement>("[data-kind]").forEach((button) => {
    button.addEventListener("click", () => {
      kind = button.dataset.kind as RunKind;
      content.querySelectorAll("[data-kind]").forEach((item) => item.classList.remove("selected"));
      button.classList.add("selected");
    });
  });
  content.querySelector<HTMLButtonElement>("#run")?.addEventListener("click", async () => {
    const target = value("#target");
    const trustAnchor = value("#trust");
    const input = value("#input");
    output = "";
    setRunState("Starting…");
    try {
      await api.run({
        kind,
        target,
        trustAnchor,
        input,
        nodeAddresses: [],
        executionEnvironment: value("#environment"),
        service: value("#service"),
        method: value("#method"),
        assurance: value("#assurance") === "appleAppAttest" ? "appleAppAttest" : "producerSigned",
        appleAppId: value("#apple-app-id"),
        appleCdHashes: value("#apple-cdhashes").split(",").map((item) => item.trim()).filter(Boolean),
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
    ? "Offer OpenAI Responses through your attested Hellas identity."
    : "Expose an authenticated OpenAI-compatible endpoint only on this machine.";
  content.innerHTML = `
    <header><p class="eyebrow">${service}</p><h1>${title}</h1><p>${description}</p></header>
    <section class="panel service-card">
      <div><div class="panel-title">Runtime</div><h2 id="service-state">${escapeHtml(current?.state ?? "stopped")}</h2>
      <p id="service-detail">${escapeHtml(current?.detail ?? "Loading…")}</p></div>
      <button id="toggle" class="primary">${current?.state === "running" ? "Stop" : "Start"}</button>
    </section>
    ${service === "provider" ? providerForm() : gatewayForm()}`;
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
    <label>Allowed caller public keys<textarea id="provider-callers" rows="3" placeholder="one compressed secp256k1 key per line"></textarea></label>
    <div class="field-row"><label>Port<input id="provider-port" type="number" min="1" max="65535" placeholder="automatic" /></label></div>
    <p class="muted">The API key crosses the typed IPC boundary once and remains only in native memory for this provider run.</p></section>`;
}

function providerConfig(): ProviderConfig {
  const rawPort = value("#provider-port");
  return {
    service: value("#provider-service"),
    method: value("#provider-method"),
    openaiApiKey: value("#provider-api-key"),
    allowedCallers: value("#provider-callers").split(/[,\n]/).map((item) => item.trim()).filter(Boolean),
    port: rawPort ? Number(rawPort) : undefined,
  };
}

function gatewayForm(): string {
  return `<section class="panel"><div class="panel-title">Local access</div>
    <div class="field-row"><label>Provider endpoint<input id="gateway-target" placeholder="Hellas endpoint ID" /></label>
    <label>Provider genesis<input id="gateway-trust" placeholder="trusted content ID" /></label></div>
    <div class="field-row"><label>Execution environment<input id="gateway-environment" placeholder="Fetch manifest content ID" /></label>
    <label>Assurance<select id="gateway-assurance"><option value="producerSigned">Producer signed</option><option value="appleAppAttest">Apple App Attest</option></select></label></div>
    <div class="field-row"><label>Fetch service<input id="gateway-service" value="openai" /></label>
    <label>Fetch method<input id="gateway-method" value="responses" /></label></div>
    <div class="field-row"><label>Apple app ID<input id="gateway-apple-app-id" placeholder="TEAMID.ai.hellas.gate" /></label>
    <label>Allowed CDHashes<input id="gateway-apple-cdhashes" placeholder="hex, comma separated" /></label></div>
    <div class="field-row"><label>Bind address<input value="127.0.0.1 (ephemeral port)" readonly /></label>
    <label>Backend<input value="Verified sealed Fetch Responses" readonly /></label></div>
    <p class="muted">A fresh bearer is generated for each running instance.</p>
    ${gatewayAccess ? `<div class="definition"><div><span>Base URL</span><code>${escapeHtml(gatewayAccess.address)}</code></div><div><span>Bearer</span><code>${escapeHtml(gatewayAccess.bearer)}</code></div></div>` : ""}</section>`;
}

function gatewayConfig(): RunRequest {
  return {
    kind: "fetch",
    target: value("#gateway-target"),
    nodeAddresses: [],
    input: "{}",
    trustAnchor: value("#gateway-trust"),
    service: value("#gateway-service"),
    method: value("#gateway-method"),
    executionEnvironment: value("#gateway-environment"),
    assurance: value("#gateway-assurance") === "appleAppAttest" ? "appleAppAttest" : "producerSigned",
    appleAppId: value("#gateway-apple-app-id"),
    appleCdHashes: value("#gateway-apple-cdhashes").split(",").map((item) => item.trim()).filter(Boolean),
  };
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
    <div><span>Caller public key</span><code>${escapeHtml(identity?.callerPublicKey ?? "")}</code></div>
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
