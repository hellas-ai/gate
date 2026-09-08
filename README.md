# Hellas Gate

Gate is the small native desktop host for Hellas. It is one Tauri process and
one vanilla TypeScript UI. There is no companion daemon, internal HTTP control
plane, browser-side Rust, or second implementation of the Hellas protocol.

The native process wires together the reusable pieces in `../hellas`:

- a verified sealed-Fetch client with a stable caller and transport identity;
- an Apple App Attest OpenAI Responses provider;
- a bearer-protected `/v1/responses` gateway bound to loopback;
- private SQLite execution history; and
- a same-user Unix control socket carrying the existing Hellas mux and the
  `HostControl` protobuf service.

The only platform-specific protocol producer is the DeviceCheck bridge in
`src-tauri/src/apple_app_attest.m`. The generic attestation boundary and the
portable Apple verifier live in `hellas-attestation`. Provider and gateway
runtimes live behind `hellas-sdk`; Gate contains only lifecycle and UI policy.

The Rust/TypeScript seam is deliberately local and narrow. Rust DTOs derive
TypeScript definitions with `ts-rs`, while `ui/src/api.ts` is the sole module
allowed to invoke Tauri commands. The reusable non-UI seam is the `HostControl`
protobuf service over any Hellas transport, including the local Unix transport.
There is no npm package and no package publication in this repository.

## Local development

The flake is the canonical environment. It supplies Rust, Tauri, TypeScript,
esbuild, and Node directly; there is no npm manifest, `npm install`, or
`node_modules` directory. Rust and frontend build output both stay under
`/tmp`, off this shared checkout:

```sh
nix develop
make check
make dev
```

The Hellas crates are local path dependencies from `../hellas`. Gate's old
router, daemon, relay, TLS forwarder, and Rust/Wasm frontend have intentionally
been removed; equivalent protocol behavior must be added to Hellas rather than
copied back into this app.

## Local data and secrets

Gate stores its stable identity, App Attest enrollment, assertion counters,
provider quota/transcript state, and history below Tauri's private application
data directory. Identity files and the Unix socket are owner-only. The OpenAI
API key crosses the typed Tauri IPC seam once and is retained only in native
memory for the active provider run. The loopback gateway generates a fresh
bearer whenever it starts.

## Apple App Attest

The ordinary desktop build carries no restricted entitlement, so it launches
without an Apple provisioning profile and reports App Attest as unavailable.
An attested provider requires a separately provisioned and signed build whose
profile matches the app bundle identifier and grants the App Attest `CDhash`
entitlement. Gate never substitutes software assurance. See
`docs/SIGNING.md` for the two build modes and their provisioning boundary.
