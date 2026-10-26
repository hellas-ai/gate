# Hellas Gate

Gate is a Tauri desktop application for Fetch work. One SDK `HostHandle` owns
local control, provider and gateway lifecycles. Run and gateway requests share
SDK clients and their durable channels; paid roles share one lazy chain node.

A person has a P-256 passkey principal. Each installation has separate node and
transport keys; provider attestation has its own key. The current explicit
software authenticator is for development: assertions prove user presence, not
user verification. Native passkey and provisioned App Attest runtime acceptance
are required before a desktop release. Gate never silently substitutes software
assurance for an Apple policy.

## Develop and verify

The locked Nix shell supplies Rust, Tauri, TypeScript, esbuild and Node. There is
no npm installation. Build output stays in `target/` unless
`CARGO_TARGET_DIR` is set.

```sh
nix develop -c make check
nix develop -c make dev
```

`make check` checks the Cargo/Nix Hellas pin, regenerates TypeScript bindings,
builds and tests the shipped UI bundle, and runs formatting, strict Clippy and
Rust tests. The optional real-chain acceptance starts six local validators and
uses actual SDK execution, passkey-authorized delegation and funded channels:

```sh
nix develop -c cargo test --locked --features chain-tests --lib paid_preview_provision_and_http_work_use_real_validators_and_delegated_coins
```

Linux can compile the Windows-specific code with the checked-in MinGW shell:

```sh
nix develop .#windows-cross -c cargo check --locked --workspace --all-targets --target x86_64-pc-windows-gnu
```

This is a compile check, not a Windows desktop runtime test. CI also builds a
macOS ARM64 app. See [signing and provisioning](docs/SIGNING.md) for the signed
application pipeline.

## Identity, control and custody

First launch explicitly creates the development passkey identity. Restart uses
the same prepared app credential and pinned TLS certificate. Local Unix sockets
and Windows pipes require a scoped credential and device proof; filesystem
ownership does not grant administration. Settings can issue device-bound CLI
access after fresh owner approval. Offline owner recovery is explicit, advances
the authority generation and invalidates earlier credentials. A failed service
open can be retried without recovering or replacing the owner.

All private material uses the SDK OS secret store. An unavailable or locked store
is an error. Private files are used only by isolated headless tests. Public
configuration, verified cards, offers, trust approvals, replay counters, channel
journals and history remain in the private application data directory. Upstream
keys are write-only in the UI: it shows their label, fingerprint and reference.
History and browser state never store access or upstream secrets.

## Serve, contacts and Run

Bind the provider with owner approval, add exact upstream hosts and key scopes,
and configure OpenAI Responses or HTTPS resources. OpenAI keys are restricted to
its public Responses endpoint. HTTPS private addresses and private TLS roots
require explicit settings. Compatible resources can share a route; coupled
changes use one checked catalogue publication. Invalid settings are refused
before retiring the live provider. One host serves authorized and paid work.

Import a signed ContactCard, compare its fingerprint independently and explicitly
confirm the recipient's device node key. Shares select per-upstream allowances
and retain their budget ancestry. AccessBundle v2 includes all selected offers
and a device-bound credential; import checks both person and device. Trust is
approved separately in the SDK TrustPolicyStore with independently obtained
provider genesis/environment pins and, when required, Apple app-ID/CDHash pins.

All access forms use the same fixed lifetime choices: 1 day, 1 week (the default),
30 days, 365 days or Unlimited (100 years). Revisions and counters survive normal
restart and trust changes. Pause, quota editing and revocation use the existing
Work/Auth authority operations.

Run selects imported access or a named typed paid pool, then accepts the actual
Fetch request JSON. Successful terminal verification, failure and cancellation
are reflected in local SQLite history. History entries can be deleted or cleared.
Closing the window hides it; Quit drains the host, clients and chain. Saving a
host UDP port takes effect on an explicit drain/restart.

## Paid setup and gateways

Paid Serve and pools use typed policy, validator, funding and route fields.
Preview derives the bond from the live authority owner and a verified client
without signing, spending or writing a journal. Provisioning requires a stopped,
drained provider because setup journals have one exclusive owner. Stake must be
owned by that authority owner and delegated to the provider node. Review the
coin IDs, expiry, returned value and payout delegation before signing the offer.
The SDK journals reservations before exporting; the client handshake performs
the on-chain spend. Use the paid pool form’s client transport handoff for its bilateral route (each
pool keeps a stable transport ID). Add the resulting bond/client route and start
the provider.

The loopback gateway supports Responses with authorized or paid funding, and
HTTP with paid funding. Named `gateway.invoke` credentials are issued, rotated
and revoked through the authority. Interrupted preparation appears as pending;
an owner can reconcile the exact retained credential or cancel it. Expired or
revoked credentials cannot invoke work. No gateway key grants node control.

Gateway forms choose archive policy explicitly. ZDR is enabled by default;
otherwise request/response bodies are archived under the private application
state. Gateway reconfiguration and shutdown drain accepted work.

Settings can preview and apply a typed desired-state document. It uses existing
vault references for secrets and the same native operations as interactive
forms. Preview captures the configuration revision; stale previews refuse.
Applies are sequential: failures report acknowledged effects, unapplied work
and any uncertain operation separately. Reapplying converges without resetting
budgets, credentials or journal history.
