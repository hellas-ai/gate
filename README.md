# Hellas Gate

Gate is a Tauri desktop application for Fetch work. One SDK `Core` owns
the journaled configuration, local control, provider and HTTP listeners. Run and gateway requests share
SDK `ExecutorGateway` targets and their durable channels. Paid roles use verified
light-client reads against the selected chain profile.

Read [the privacy and trust boundaries](PRIVACY.md) before sharing an account or
approving a provider.

A person has a P-256 passkey principal. Each installation has separate node and
transport keys; provider attestation has its own key. On macOS, Gate uses AuthenticationServices to create a system passkey and request
user verification for owner approvals. Gate stores its public key and credential
identifier, never its private key. The explicit software authenticator remains
for development: assertions prove user presence, not user verification. Signed
macOS passkey and provisioned App Attest runtime acceptance remain release checks. Gate never silently substitutes software
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
Rust tests. The optional chain acceptance uses the SDK's six independently
executing validators. Its fixture orders blocks; signatures, execution,
replication and funded SDK channels remain real:

```sh
nix develop -c make check-chain
```

Linux can compile the Windows-specific code with the checked-in MinGW shell:

```sh
nix develop .#windows-cross -c cargo check --locked --workspace --all-targets --target x86_64-pc-windows-gnu
```

This is a compile check, not a Windows desktop runtime test. CI also builds a
macOS ARM64 app. See [signing and provisioning](docs/SIGNING.md) for the signed
application pipeline.

## Identity, control and custody

First launch creates a system passkey on a signed, domain-associated macOS app.
Setup then requests one owner assertion to initialize the node. Signing a contact card is a separate, explicit Settings action for when you want to share your identity; successful exports are saved and reused without another approval. Cancelling that action leaves the initialized node usable. Gateway listener access is provisioned only when you explicitly enable Gateway, using that operation's owner-approved session; mounting or reopening Gate never asks for listener approval.
A separate development option explicitly selects a software identity on any
platform. Native prompt errors never fall back to software. Interrupted setup
reuses the saved native credential when retried. Restart uses
the same prepared app credential and pinned TLS certificate. Local Unix sockets
and Windows pipes require a scoped credential and device proof; filesystem
ownership does not grant administration. Settings can issue device-bound CLI
access after fresh owner approval. Offline owner recovery uses the installation's existing native or software
owner authenticator, is explicit, advances
the authority generation and invalidates earlier credentials. A failed service
open can be retried without recovering or replacing the owner.

Identity and imported credential custody use the SDK OS secret store. An unavailable
or locked store is an error. Test fixtures use isolated private files. The authority
journal stores the daemon configuration and backend secrets atomically in the private
application data directory. Upstream keys are write-only: Configure accepts them,
and Watch returns only the routing aliases. The editor has no separate key catalogue.
Verified cards, offers, trust approvals, replay counters, channel journals and history
also remain in the private application data directory.
History and browser state never store access or upstream secrets.

## Serve, contacts and Run

Bind the provider with owner approval, add exact upstream hosts and key scopes,
and configure OpenAI Responses or HTTPS resources. OpenAI keys are restricted to
its public Responses endpoint. HTTPS private addresses and private TLS roots
require explicit settings. Compatible resources can share a route; coupled
changes use one atomic Configure. Watch is the provider editor’s source of truth.
Invalid settings leave the previous configuration serving. One daemon serves
authorized and paid work.

Import a signed ContactCard, compare its fingerprint independently and explicitly
confirm the recipient's device node key. Shares select per-upstream allowances
and retain their budget ancestry. AccessBundle v2 includes all selected offers
and a device-bound credential; import checks both person and device. Trust is
approved separately in the SDK TrustPolicyStore with independently obtained
provider enrollment pins and, when required, Apple app-ID/CDHash pins.

All access forms use the same fixed lifetime choices: 1 day, 1 week (the default),
30 days, 365 days or Unlimited (100 years). Revisions and counters survive normal
restart and trust changes. Pause, quota editing and revocation use the existing
Work/Auth authority operations.

Run selects imported access or a named typed paid pool, then accepts the actual
Fetch request JSON. Successful terminal verification, failure and cancellation
are reflected in local SQLite history. History entries can be deleted or cleared.
Closing the window hides it; Quit drains the host and clients. Saving a
provider UDP port takes effect on the next explicit stop/start of Serve. Restart
restores the active listener configuration from the journal.

## Paid setup and gateways

Choose the compiled devnet profile or paste custom chain-profile JSON
containing `trust`, `validators` and `genesis_json`. Gate saves these contents in
the native Run pool or daemon listener configuration. The SDK validates the genesis and trust schedule and
keeps each chain’s state in its own directory.

Start Serve with a configured Fetch resource, then approve one standing paid
policy with the owner passkey: price, jobs per channel, per-job limits, maximum
stake per client, maximum open offers, and bond lifetime. The authority retains
the approved policy and generates its salt. The node offers clients bonds on
demand using owner-owned coins already delegated to this provider node; each
stake coin must fit the configured cap. Stopping Serve stops its Iroh listener,
including authorized and paid serving; existing channels retain their signed terms.

A requester selects an `ENROLLMENT_ID@NODE_ID` provider address (shown in Serve),
independently approved trust, named route and
price. Quote discovery does not fund a channel. The first accepted request asks
the owner to approve an exact funding split; the SDK retains funding and channel
state for recovery. Each provider belongs to one paid pool. Reuse that pool for
retained work. Native Run and HTTP use the daemon’s outgoing transport identity; retained journals
are scoped to the chain and full approved provider address. The address's
enrollment must match the approval; the SDK verifies it on quote, funding and
reconnect, including retained channels. Node IDs alone are rejected.

Run and listeners share the configured SDK gateway backend and its retained channel.
While an HTTP gateway is configured for the selected provider, Run uses that
backend’s channel timing and limits. Native pool timing applies to standalone Run. Gateway jobs use the CLI defaults
of 300 admission, 3,000 execution and 600 payment blocks; a bond must have more
than their combined 3,900 blocks remaining. Paid Serve defaults to 10,000 blocks. They select a named route and
HTTP or Responses presentation. The SDK currently prepares the Fetch contract
from the grant's admitted policy or the paid provider's quoted policy, refusing
ambiguous admissions. These paths do not independently pin a buyer-named
contract. There is no separate environment allowlist. The SDK's job API accepts
an explicit buyer contract, but its gateway API does not yet expose that choice.

The loopback gateway supports Responses with authorized or paid funding, and
HTTP with either funding mode. Named `gateway.invoke` credentials are issued, rotated
and revoked through the authority. Interrupted preparation appears as pending;
an owner can reconcile the exact retained credential or cancel it. Expired or
revoked credentials cannot invoke work. No gateway key grants node control.

Gateway forms choose archive policy explicitly. ZDR is enabled by default;
otherwise request/response bodies are archived under the private application
state. Gateway reconfiguration preserves admitted work on retained backends; shutdown
drains accepted work.

Settings can preview and apply a typed desired-state document. It accepts write-only backend keys and uses the same operations as interactive
forms. Omit a secret value to keep its existing backend key. Preview captures the configuration revision; stale previews refuse.
Applies are sequential: failures report acknowledged effects, unapplied work
and any uncertain operation separately. Reapplying converges without resetting
budgets, credentials or journal history.

Gate's application-menu Quit (Cmd-Q on macOS) and tray Quit wait for the local
host to shut down before exiting. macOS Dock Quit and OS termination can bypass
this asynchronous shutdown path, as can a force quit. On relaunch, the local
control listener safely recovers a stale socket while refusing an active listener;
the identity and other persisted state are retained.
