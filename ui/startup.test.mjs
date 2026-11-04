import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import vm from "node:vm";

// Run the shipped bundle against the Tauri/DOM boundary needed for initial boot.
test("a rejected first status call displays its cause on an existing error surface", async () => {
  const nodes = new Map();
  const root = {
    html: "",
    set innerHTML(value) {
      this.html = value;
      if (value.includes('id="error"')) nodes.set("#error", { textContent: "" });
    },
    get innerHTML() { return this.html; },
    addEventListener() {},
    querySelectorAll() { return []; },
    append(node) { nodes.set(`#${node.id}`, node); },
  };
  nodes.set("#app", root);
  let requests = 0;
  let errorSurfacePrecededRequest = false;
  const cause = "Fixture: local authority TLS/credential unavailable";
  const context = vm.createContext({
    document: {
      querySelector(selector) { return nodes.get(selector) ?? null; },
      createElement() { return { setAttribute() {}, textContent: "" }; },
    },
    window: { __TAURI__: { core: {
      Channel: class {},
      async invoke(command) {
        assert.equal(command, "get_status");
        requests++;
        errorSurfacePrecededRequest = nodes.has("#error");
        throw { code: "unavailable", message: cause };
      },
    } } },
    setInterval() { return 0; },
  });
  const script = await readFile(new URL("../target/ui-dist/main.js", import.meta.url), "utf8");
  vm.runInContext(script, context);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(requests, 1);
  assert.equal(errorSurfacePrecededRequest, true);
  assert.equal(nodes.get("#error")?.textContent, cause);
  assert.match(root.innerHTML, /Retry connection/);
});

test("Gateway renders issued and pending keys with recovery actions", async () => {
  const nodes = new Map();
  const listeners = new Map();
  const root = {
    html: "",
    set innerHTML(value) {
      this.html = value;
      for (const id of ["error", "content", "rail-status", "message"]) {
        if (value.includes(`id="${id}"`)) nodes.set(`#${id}`, { innerHTML: "", textContent: "" });
      }
    },
    get innerHTML() { return this.html; },
    addEventListener(name, handler) { listeners.set(name, handler); },
    querySelectorAll() { return []; },
    append(node) { nodes.set(`#${node.id}`, node); },
  };
  nodes.set("#app", root);
  const context = vm.createContext({
    document: {
      querySelector(selector) { return nodes.get(selector) ?? null; },
      createElement() { return { setAttribute() {}, textContent: "" }; },
    },
    crypto: { getRandomValues(value) { value.fill(1); return value; } },
    window: { __TAURI__: { core: {
      Channel: class {},
      async invoke(command) {
        if (command === "get_status") return {
          identity: { initialized: true }, provider: { state: "stopped" }, gateway: { state: "stopped" },
        };
        if (command === "list_access" || command === "list_trust") return [];
        if (command === "list_gateway_keys") return [
          { id: "ab".repeat(16), label: "Issued tool", expires: 2000000000000, state: "active" },
          { id: "cd".repeat(16), label: "Pending tool", expires: 2000000000000, state: "pending" },
        ];
        throw new Error(`unexpected Tauri command: ${command}`);
      },
    } } },
    setInterval() { return 0; },
  });
  vm.runInContext(await readFile(new URL("../target/ui-dist/main.js", import.meta.url), "utf8"), context);
  await new Promise(resolve => setImmediate(resolve));
  listeners.get("click")({ target: { closest() { return { dataset: { view: "gateway" } }; } } });
  await new Promise(resolve => setImmediate(resolve));
  const html = nodes.get("#content")?.innerHTML;
  assert.match(html, /Issued tool/);
  assert.match(html, /Pending tool/);
  assert.match(html, /Reconcile prepared key/);
  assert.match(html, /Cancel preparation/);
  assert.equal(nodes.get("#error")?.textContent, "");
});

test("Paid Serve publishes standing terms only after owner approval", async () => {
  const nodes = new Map(), listeners = new Map(), dialogs = [], changes = [];
  const root = {
    set innerHTML(value) {
      for (const id of ["error", "content", "rail-status", "message"]) {
        if (value.includes(`id="${id}"`)) nodes.set(`#${id}`, { innerHTML: "", textContent: "" });
      }
    },
    addEventListener(name, handler) { listeners.set(name, handler); },
    querySelectorAll() { return []; },
    append(node) { nodes.set(`#${node.id}`, node); },
  };
  nodes.set("#app", root);
  const values = {
    network: "fixture", genesisDigest: "11".repeat(32), thresholdIdentity: "22".repeat(48),
    validators: "ws://localhost:1", service: "web", method: "get", presentation: "http",
    fixedPrice: "10", pollMs: "50", maxObservationAgeMs: "30000", jobsPerChannel: "10",
    omissionBond: "101", maxStakePerClient: "1000", maxOpenOffers: "4", bondLifetimeBlocks: "2000",
    redelegatePayout: "on", computeCreditLimit: "40", deliveryCreditLimit: "40",
    dispatchBlocks: "4", deliveryBlocks: "2", oracleBlocks: "6", minOmitResponseBlocks: "16",
    maxRequestBodyBytes: "65536", maxOutputEvents: "4096", maxOutputBytes: "4194304",
    maxSpoolBytes: "8388608", maxEncodedResultFrame: "4194304", maxEncodedPreparedInput: "131072",
    executionTimeoutMs: "300000",
  };
  const context = vm.createContext({
    document: {
      querySelector(selector) { return nodes.get(selector) ?? null; },
      body: { append() {} },
      createElement(tag) {
        assert.equal(tag, "dialog");
        return { querySelector() { return { textContent: "" }; }, showModal() { dialogs.push(this); },
          addEventListener(_name, handler) { this.close = handler; }, remove() {} };
      },
    },
    FormData: class {
      get(name) { return values[name] ?? ""; }
      getAll() { return []; }
      has(name) { return Object.hasOwn(values, name); }
    },
    window: { __TAURI__: { core: {
      Channel: class {},
      async invoke(command, args) {
        if (command === "get_status") return { identity: { initialized: true },
          provider: { state: "running" }, gateway: { state: "stopped" }, minOmitResponseBlocks: 16 };
        if (command === "provider_view") return { revision: 7, resources: [], upstreams: [], paid: null };
        if (command === "configure_paid_serve") { changes.push(args); return {}; }
        throw new Error(`unexpected command ${command}`);
      },
    } } },
    setInterval() { return 0; },
  });
  const flush = () => new Promise(resolve => setImmediate(resolve));
  vm.runInContext(await readFile(new URL("../target/ui-dist/main.js", import.meta.url), "utf8"), context);
  await flush();
  listeners.get("click")({ target: { closest() { return { dataset: { view: "serve" } }; } } });
  await flush();
  assert.match(nodes.get("#content").innerHTML, /Maximum provider stake per client/);
  assert.doesNotMatch(nodes.get("#content").innerHTML, /id="provision"|name="routeBond"|name="stakeCoins"/);
  const submit = () => listeners.get("submit")({ target: { id: "paid-serve" }, preventDefault() {} });
  submit(); await flush();
  assert.equal(changes.length, 0, "the policy cannot be submitted before review");
  assert.equal(dialogs.length, 1);
  assert.match(dialogs[0].innerHTML, /Continue to passkey/);
  dialogs[0].returnValue = "approve"; dialogs[0].close(); await flush();
  assert.equal(changes.length, 1);
  assert.equal(changes[0].expected, 7);
  assert.equal(changes[0].input.policy.maxStakePerClient, 1000);
  assert.equal(changes[0].input.policy.limits.maxOutputBytes, 4194304);
  assert.equal(changes[0].input.policy.redelegatePayout, true);
  assert.equal("routes" in changes[0].input, false);
  assert.equal("policySalt" in changes[0].input.policy, false);
  submit(); await flush();
  dialogs[1].returnValue = "cancel"; dialogs[1].close(); await flush();
  assert.equal(changes.length, 1, "cancelling owner approval must not publish");
  assert.equal(nodes.get("#error").textContent, "");
});

test("setup defaults to native passkeys and never falls back after a cancelled prompt", async () => {
  const nodes = new Map(), listeners = new Map(), dialogs = [], calls = [];
  const values = { name: "Alice" };
  const root = {
    set innerHTML(value) {
      for (const id of ["error", "content", "rail-status", "message"])
        if (value.includes(`id="${id}"`)) nodes.set(`#${id}`, { innerHTML: "", textContent: "" });
    },
    addEventListener(name, handler) { listeners.set(name, handler); },
    querySelectorAll() { return []; },
    append(node) { nodes.set(`#${node.id}`, node); },
  };
  nodes.set("#app", root);
  const context = vm.createContext({
    document: {
      querySelector(selector) { return nodes.get(selector) ?? null; },
      body: { append() {} },
      createElement() { return {
        querySelector() { return { textContent: "" }; },
        showModal() { dialogs.push(this); },
        addEventListener(_name, handler) { this.close = handler; }, remove() {},
      }; },
    },
    FormData: class {
      get(name) { return values[name] ?? ""; }
      has(name) { return Object.hasOwn(values, name); }
    },
    window: { __TAURI__: { core: {
      Channel: class {},
      async invoke(command) {
        if (command === "get_status") return { identity: { initialized: false, detail: "Set up Gate" },
          provider: { state: "stopped" }, gateway: { state: "stopped" }, boundAddresses: [] };
        calls.push(command);
        if (command === "initialize_native") throw { message: "Passkey authorization cancelled" };
        if (command === "initialize_software") return {};
        throw new Error(`unexpected command ${command}`);
      },
    } } },
    setInterval() { return 0; },
  });
  const flush = () => new Promise(resolve => setImmediate(resolve));
  vm.runInContext(await readFile(new URL("../target/ui-dist/main.js", import.meta.url), "utf8"), context);
  await flush();
  assert.match(nodes.get("#content").innerHTML, /name="software" type="checkbox">/);
  const submit = () => listeners.get("submit")({ target: { id: "initialize" }, preventDefault() {} });
  submit(); await flush();
  dialogs[0].returnValue = "approve"; dialogs[0].close(); await flush();
  assert.deepEqual(calls, ["initialize_native"]);
  assert.equal(nodes.get("#error").textContent, "Passkey authorization cancelled");
  values.software = "on";
  submit(); await flush();
  dialogs[1].returnValue = "approve"; dialogs[1].close(); await flush();
  assert.deepEqual(calls, ["initialize_native", "initialize_software"]);
});
