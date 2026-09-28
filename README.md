# Hellas Gate

Gate is the small native desktop host for Hellas. It is one Tauri process and
one vanilla TypeScript UI. There is no companion daemon, internal HTTP control
plane, browser-side Rust, or second implementation of the Hellas protocol.

The native process wires together the reusable pieces in `../hellas`:

- a verified Fetch client with a stable caller and transport identity;
- an Apple App Attest provider for generic HTTPS and sealed OpenAI Responses;
- paid Fetch channel provisioning, execution, payment and settlement;
- a bearer-protected paid HTTP gateway bound to loopback;
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
Rust and frontend build output both stay under `/tmp`, off this shared checkout.
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
checked-in lockfile. Gate's old router, daemon, relay, TLS forwarder, and
Rust/Wasm frontend have intentionally been removed; equivalent protocol
behavior must be added to Hellas rather than copied back into this app.

## Local data and secrets

Gate stores its stable identity, App Attest enrollment, assertion counters,
provider quota metadata, and history below Tauri's private application
data directory. Identity files and the Unix socket are owner-only. The OpenAI
API key crosses the typed Tauri IPC seam once and is retained only in native
memory for the active provider run. The loopback gateway generates a fresh
bearer whenever it starts.

Incoming provider requests use zero retained-transcript capacity: customer
prompt and response bodies stay in memory, and requests asking for retention
are refused before execution. Gate's own Run history remains local client data.
The provider cannot replay an ephemeral response after restart. This covers
Gate's application storage, not the upstream API's retention policy.
The sealed Responses driver discards unsuccessful upstream bodies without
reading or logging them. Generic HTTPS returns the signed status and body to
the requesting client, including non-success statuses, without logging bodies.
Paid provider journals persist hashes, signatures and payment/settlement state,
and omit request and response bodies from both appends and checkpoints.

An older provider directory configured with nonzero transcript capacity fails
to open under this policy. The SDK requires all users of that directory to be
stopped before its capacity metadata is changed. Existing transcripts are not
automatically deleted or converted; they need a separate migration decision.

## Apple App Attest

The ordinary desktop build carries no restricted entitlement, so it launches
without an Apple provisioning profile and reports App Attest as unavailable.
An attested provider requires a separately provisioned and signed build whose
profile matches the app bundle identifier and grants the App Attest `CDhash`
entitlement. Gate never substitutes software assurance. See
`docs/SIGNING.md` for the two build modes and their provisioning boundary.

## Generic HTTPS and paid Fetch

See [HTTPS requests and account aliases](../hellas/crates/providers/HTTPS.md)
for the signed request schema and operator account configuration. Select HTTPS
on Serve; use `http` as the environment on Run. Several aliases may use the
same origin with different account environment variables. The app does not
need a compiled list of upstream vendors.

See [paid Fetch](../hellas/crates/work/README.md) for the policy fields and ZDR
recovery rules. Gate uses those shared SDK implementations. Configure the
chain identity, validators, funding, journal root and bilateral routes in a
work config, with `policies.fetch` selecting the HTTP manifest.

Gate can create its own provider offer from a JSON file:

```json
{
  "work_config": "/absolute/path/provider-work.json",
  "client": "<client compressed secp256k1 key, 66 hex digits>",
  "stake_coins": ["<provider funding coin, 64 hex digits>"],
  "bond_timeout": 10000,
  "timeout_payout": 1000,
  "max_job_price": 10
}
```

The amounts and height are examples; use the intended channel's funding and
terms. In Serve, enter this file as **Provider offer file** and choose **Preview
bond**. Add the returned bond, the client's transport peer and settlement key
to the work config's bilateral routes. **Create offer** then signs and journals
the offer with this Gate installation's identity. Set **Paid provider
configuration** to that work config and start the provider. Stop it before
provisioning another offer. Existing offers and funding collisions are checked
before signing; a second offer cannot reuse the first offer's peer or stake.

On the requesting Gate installation, make a separate paid client config:

```json
{
  "work_config": "/absolute/path/provider-policy-copy.json",
  "journal_root": "/absolute/path/client-paid-journals",
  "bond": "<provider bond, 64 hex digits>",
  "payment_coins": ["<client funding coin, 64 hex digits>"],
  "omission_bond": 10,
  "settle": false
}
```

Set **Paid client configuration** on Run. Select Apple App Attest and provide
the out-of-band provider enrollment pin, app ID and allowed CDHashes. Both paid
services authenticate a fresh connection-bound Open proof before receiving
requests; the attested producer must match the channel's provider. Gate verifies
the signed response before paying and completes the run after the payment
acknowledgement. Set `settle` to true to also wait for finalized settlement.

A failed client run leaves its signed pending request in its own private journal
directory so a retry uses the same nonce. Resume with the same request and
configuration. Provider restarts cannot recover response bodies or silently
repeat upstream work. The provider can still credit an already-delivered
result and recover settlement from retained payment metadata. Client-owned
request journals and Run history are separate from provider storage.

## Paid loopback gateway

The Gateway view uses the same SDK paid pool as `hellas-cli gateway`. Supply
absolute paths to a [paid provider pool](../hellas/docs/paid-gateway.md) and
[HTTP route configuration](../hellas/docs/http-gateway.md), choose the required
provider assurance, and start it. Provider enrollment and Apple trust pins
belong in the pool file. Its payment coins must belong to Gate's displayed
caller settlement identity, and the provider must authorize Gate's transport
identity. A missing pool or an unfunded channel cannot fall back to Courtesy.

The UI exposes the bound loopback URL and fresh bearer for coding clients.
Routing, session affinity, quota/backoff handling and payment recovery run in
Hellas. Stopping the listener drains accepted work; unfinished payments retain
metadata for recovery. Ordinary exchanges archive under `gateway-archive` in
Gate's data directory. Zero data retention disables payload archiving. Archive
failures are reported by the gateway and do not interrupt requests.

Provider API keys remain provider-owned: HTTP routes name credential aliases;
the provider injects the scoped header at HTTPS egress. See the
[credential demonstration](../hellas/crates/providers/HTTPS.md#provider-owned-credential-demonstration).
