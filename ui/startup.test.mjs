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
