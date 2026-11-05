# How do I know this is private?

Hellas builds trust for deterministic work through re-execution, bonds and
cryptography. Fetching an SSL API is about trust: a trusted friend can share
with a grant through `hellas serve`. To buy from a stranger, B independently
approves the audited Gate build and uses Apple App Attest to verify its identity.

The shipping `ai.hellas.gate` attested paid journey is **UNTESTED**. Earlier live
evidence used a separate spike bundle; software-signed work is not Apple evidence.

## Plaintext and credentials

```text
B -- encrypted Hellas connection --> A's Gate -- HTTPS --> API service
              blind relay             plaintext          plaintext
```

The relay lacks the endpoint keys, but sees timing and sizes. Gate and the API
service process plaintext; B receives the result. App Attest does not turn Gate
memory into an enclave or protect against every compromised-host or approved-code
vulnerability. [Run history](src-tauri/src/history.rs) and optional archives can
persist local plaintext. ZDR does not erase backups, swap or vendor records.

A's credential stays in its referenced secret store, environment or private file.
The driver injects it only for its configured origin, paths and methods. B gets
scoped access, not the key. A malicious or credential-reflecting upstream can still
disclose it. Codex reads native `auth.json` read-only; the external Codex CLI is the
sole refresher, and expired tokens fail closed.

Code: [credential scope](https://github.com/hellas-ai/hellas/blob/8ccff0a3a90dc2e5612e1482cf60432df5cf893e/crates/providers/src/http.rs),
[local references](src-tauri/src/vault.rs),
[native Codex auth](https://github.com/hellas-ai/hellas/blob/8ccff0a3a90dc2e5612e1482cf60432df5cf893e/crates/sdk/src/codex_auth.rs).

## What Apple verification establishes

B checks the certificate chain to Apple's public App Attestation Root CA, App ID
hash, key ID, approved CDHash and a fresh signed assertion. The assertion binds
the enrolled producer/transport keys to the live QUIC connection, challenge and
service. Persistent counters reject replay. The CLI reports verified identity,
challenge and counter fields before disclosing request data.

B must obtain the expected enrollment, App ID and full build hash independently
of A. App Attest identifies an executable; it does not audit it, prove an API's
claims or freeze mutable configuration. The release audit/build process must
connect that hash to reviewed source. No independently reproducible signed build
mapping has been demonstrated here. Developer ID signature inspection and remote
App Attest verification are related, distinct checks.

Code: [DeviceCheck bridge](src-tauri/src/apple_app_attest.m),
[key/enrollment](src-tauri/src/apple.rs),
[client verification](https://github.com/hellas-ai/hellas/blob/8ccff0a3a90dc2e5612e1482cf60432df5cf893e/crates/client/src/iroh.rs),
[Apple verifier](https://github.com/hellas-ai/hellas/blob/8ccff0a3a90dc2e5612e1482cf60432df5cf893e/crates/attestation/src/apple.rs).
The verifier does **not** validate the embedded fraud-assessment receipt or call
Apple's fraud service. Captured objects test verification at capture time; only a
live provisioned Mac can answer a new hardware challenge.

## TLS when A controls the Mac

A can change DNS, proxy variables and the system trust store. The Fetch clients
use [bundled WebPKI roots](https://github.com/hellas-ai/hellas/blob/8ccff0a3a90dc2e5612e1482cf60432df5cf893e/crates/providers/src/http/tls.rs),
disable proxies and redirects, and check the TLS hostname. Installing a system
CA does not add an upstream trust anchor. Generic HTTP's explicit private roots
must match B's requested roots; a credential cannot silently replace them.

B can sign a hostname and allowed SPKI set for HTTP, OpenAI and Codex. Gate checks
the host before connecting and the SPKI during TLS, before HTTP data/credentials.
After the handshake, its signed response reports the actual peer host and leaf
SPKI. The earlier App Attest connection proof cannot attest a future upstream
handshake. A software-only provider can fabricate this claim; B must trust A.
Public CA issuance and independently approved pins remain trust assumptions.

Code: [HTTP client](https://github.com/hellas-ai/hellas/blob/8ccff0a3a90dc2e5612e1482cf60432df5cf893e/crates/providers/src/http/clients.rs),
[Responses client](https://github.com/hellas-ai/hellas/blob/8ccff0a3a90dc2e5612e1482cf60432df5cf893e/crates/providers/src/responses_fetch.rs),
[signed policy/peer](https://github.com/hellas-ai/hellas/blob/8ccff0a3a90dc2e5612e1482cf60432df5cf893e/crates/rpc/src/upstream.rs).

## What to check

Independently review the enrollment, App ID, release hash, admitted contract,
URL/TLS terms and resource limits. For the final signed app:

```sh
codesign --verify --deep --strict --verbose=2 "$app"
codesign --display --verbose=4 "$app"
codesign --display --entitlements :- "$app"
```

Compare the signing authority and 64-hex `CandidateCDHashFull sha256`, rather than
the shorter `CDHash=`. Local inspection alone does not verify a remote connection.
Require Apple assurance and the independent pins in B's client; approval alone
is not a successful live verification. See [signing and test instructions](docs/SIGNING.md)
and the [CLI usage](https://github.com/hellas-ai/hellas/blob/8ccff0a3a90dc2e5612e1482cf60432df5cf893e/README.md). macOS27+, SDK27+, SIP, Full Security and a matching
App Attest profile are required for the live Gate path.

## Limits protecting A's accounts

Gate and `hellas serve` use the same [SDK host config](https://github.com/hellas-ai/hellas/blob/8ccff0a3a90dc2e5612e1482cf60432df5cf893e/crates/sdk/src/serve_config.rs).
Credential routes explicitly bind canonical accounts; aliases share one durable
limiter below both grant and paid calls. It enforces request/output-token windows,
concurrency and a reserve from fresh exporter measurements. Missing, stale or
exhausted observations refuse sharing. Generation calls retain the full output-cap
charge after failure/cancellation; empty GET/HEAD reads still consume requests and
concurrency. Grant and paid budgets apply in addition. Measurements lag vendor
activity between polls, so the reserve is not an instantaneous vendor guarantee.

You trust Apple's root and platform enforcement, signing-key custody, the reviewed
build mapping, your client/counter storage, and the selected TLS anchors/upstream.
Attestation supplies software identity; it does not remove these assumptions.
