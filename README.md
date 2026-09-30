# Hellas Gate

Gate is the small native desktop host for Hellas. It is one Tauri process and
one vanilla TypeScript UI. There is no companion daemon, internal HTTP control
plane, browser-side Rust, or second implementation of the Hellas protocol.

The native process wires together the reusable pieces in `../hellas`:

- a verified sealed-Fetch client with a stable caller and transport identity;
- an Apple App Attest provider for OpenAI Responses and HTTPS resources;
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
`node_modules` directory. Rust 1.96.1 is shared with the pinned Hellas workspace.
Rust and frontend build output stay under this worktree’s `target/` directory.
Set `CARGO_TARGET_DIR` to override the Rust output location; the flake, Makefile,
and CI all respect it.

Gate uses local path dependencies from a sibling Hellas checkout. From a fresh
Gate clone, prepare the exact published revision recorded in `.hellas-revision`:

```sh
git clone https://github.com/hellas-ai/hellas.git ../hellas
git -C ../hellas checkout --detach "$(cat .hellas-revision)"
```

If `../hellas` already contains your work, keep it intact and create a separate
parent directory with sibling `gate` and `hellas` checkouts for the pinned build.
CI uses the same revision and layout. To update Hellas deliberately, change
`.hellas-revision`, reconcile the desktop SDK calls, and regenerate `Cargo.lock`
against that checkout.

Then run:

```sh
nix develop
make check
make dev
```

`make check` runs binding generation, frontend typechecking and production
bundling, Rust formatting, warnings-as-errors Clippy, and Rust tests with the
checked-in lockfile. Shared protocol behavior lives in Hellas.

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

### Work grants

Authorized work uses Work grants. In Settings, copy the contact
enrollment to the provider operator. The operator pastes contact enrollments in
Serve, chooses the per-contact daily Requests allowance, starts the provider, and
exports one private Offer for each contact. Paste an Offer into Run or Gateway, choose Authorized, and select its resource name (normally `responses`).
Offers must bootstrap a new session within five minutes; export again if needed.
An active session refreshes its standing automatically. Removing contacts on the
next provider start permanently revokes their grants without resetting counters.

In-app runs and the loopback gateway share one Work backend for the selected target. Stop the gateway
before changing the Offer's channel, addresses, or trust policy. Accepted work is
drained when the gateway stops. The upstream OpenAI key remains in native memory.
Gate's SQLite history remains local.

Apple provider trust requires an independently trusted app ID and CDHash allowlist.
Builds can set `HELLAS_GATE_TRUST_APP_ID` and `HELLAS_GATE_TRUST_CDHASHES` for already
approved provider releases; otherwise fill both fields from trusted release
metadata. An Offer cannot choose its own allowed software. A binary cannot embed
its own final CDHash. Live App Attest provisioning and provider execution require
a provisioned macOS build; Linux and Windows run authorized and paid clients.

Build artifacts stay under this checkout's `target/` directory, including UI
assets. Gate's provider configuration owns its contact grants.

### Paid work

Choose Paid in Run or Gateway, provide an absolute pool-file path and select a
provider endpoint ID from that pool. For an open HTTPS policy, also enter the
provider's Fetch service and method; a sealed policy supplies them. The pool uses the SDK's `load_pool_options`
format in `../hellas/docs/paid-gateway.md`: provider work config, client journal,
bond, payment coins, omission bond and mandatory `provider_genesis`. Use the
signed offer object exported by provisioning for `provider_genesis`; Gate derives
the pin and verifies Open before submitting work. Apple app ID and CDHashes for
paid targets come from the pool file; Assurance is selected in Gate. Account
credentials stay at the provider.

The active backend snapshots its configuration. Stop the gateway and switch
targets, or restart Gate, to reload edited pool/work files. Each provider owns a
separate funded channel and exclusive client journal. Accepted work drains on
shutdown; the SDK recovers durable payment obligations on the next start. In-app
runs and `/v1/responses` use the same sessions. The Responses listener requires
a resource with the OpenAI Responses manifest; Run also accepts generic HTTPS
Fetch request JSON.

To serve both fundings, set the paid Work config path alongside the contact
list. The Work config's paid Fetch policy must name a registered route. Gate
uses one provider identity, executor and Work listener; WorkSetup is added when
paid Work is configured. Fetch journals contain accounting metadata, while the
requesting user's history stores their own requests and results.

To provision a bond, stop the provider and supply a paid offer JSON file:

```json
{
  "work_config": "/absolute/path/provider-work.json",
  "client": "CLIENT_SETTLEMENT_PUBLIC_KEY_HEX",
  "stake_coins": ["PROVIDER_COIN_ID_HEX"],
  "bond_timeout": 500,
  "timeout_payout": 64,
  "max_job_price": 40,
  "addresses": ["192.0.2.10:31145"]
}
```

Preview derives the bond ID without reserving coins. Add that bond, the client's
transport endpoint and settlement key to the Work config's bilateral routes,
then choose Provision paid offer. Copy the returned `provider`, `bond` and
`provider_genesis` fields into the client's pool entry and add their payment
funding. Provisioning uses Gate's attested enrollment and its existing settlement
identity; it reserves the staked coins in the provider journal before exporting.

### HTTPS resources

Serve accepts an optional HTTPS routes JSON array beside the OpenAI API key.
Each route has `service`, `method`, `account` (the SDK `HttpProviderConfig`) and
an optional authorized `resource` with `name` and `https` (`HttpsResource`). One
resource per route keeps URL and accounting policy unambiguous. For example:

```json
[{
  "service": "lan", "method": "chat",
  "account": {"allowed_hosts": ["glm.example.com"]},
  "resource": {
    "name": "chat",
    "https": {
      "origin": "https://glm.example.com",
      "paths": ["/v1/chat/completions"], "methods": ["POST"],
      "credential": null,
      "tls": {"roots": {"mode": "web_pki"}, "spki_sha256": []},
      "accounting": "openai-chat", "max_output_tokens": 1024,
      "max_response_bytes": 65536
    }
  }
}]
```

Private addresses and private certificate roots require explicit account
configuration. Credential aliases bind allowed origins, paths and methods to a
provider-owned secret file or environment variable. Authorized resources share
the contact's daily Requests allowance and five-minute deadline. Their envelope
bounds request bodies to 64 KiB, output to 1 MiB and spooling to 4 MiB. Clients
apply the selected resource's token ceiling and streaming usage policy before
signing; providers verify it and quarantine resources with invalid usage.
