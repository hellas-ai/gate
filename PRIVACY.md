# How do I know this is private?

A requester approves a provider's identity and the work it may perform. A trusted
person can share Fetch access through a grant. Apple App Attest can additionally
verify that a remote provider uses an independently approved executable.
Attestation identifies software; reviewing that software and its configuration
remains part of deciding whether to trust it.

The shipping `ai.hellas.gate` attested paid journey has not been tested on hardware
here. Software-signed tests are not Apple evidence. The ignored
[hardware probe](docs/SIGNING.md#hardware-probe-without-ui-interaction) checks
provisioned enrollment and live assertions, not the complete paid journey.

## Plaintext and credentials

```text
Requester -- encrypted Hellas connection --> Provider's Gate -- HTTPS --> API
                    relay                       plaintext              plaintext
```

The relay lacks the endpoint keys but can observe transport metadata, timing and
sizes. Gate and the API process plaintext. App Attest does not turn Gate memory
into an enclave or protect against every compromised-host or approved-code
vulnerability. [Run history](src-tauri/src/history.rs) and optional gateway archives
can retain local request and response data. ZDR disables gateway body archival;
it does not erase Run history, backups, swap or the API vendor's records.

The provider’s upstream key is written through Configure into its private authority
journal alongside the daemon configuration. The Fetch
provider injects it only for its configured origin, paths and methods. Sharing
grants scoped access, not access to the key. A malicious or credential-reflecting
upstream can still disclose the credential. Watch returns routing aliases and redacts secret values. The editor stores no
separate key catalogue.

Code: [credential scope](https://github.com/hellas-ai/hellas/blob/a2fb38b144e10105cf75472810c2b91913374333/crates/providers/src/http.rs),
[control calls](src-tauri/src/control.rs), [provider editing](src-tauri/src/provider_setup.rs).

## What Apple verification establishes

The client verifies the enrollment certificate chain against Apple's App
Attestation Root CA, the App ID hash, key identity and approved full CDHash.
A fresh signed assertion binds the enrolled producer and transport identity to
the live QUIC connection, challenge and service. Persisted assertion counters
reject replay before work requests disclose their data.

Obtain the expected enrollment, App ID and full build hash independently of the
provider. App Attest does not audit the executable, prove the API's claims or
freeze mutable settings. The release review must connect the approved hash to
reviewed source. This repository does not demonstrate an independently
reproducible mapping from source to a signed release hash.

Code: [DeviceCheck bridge](src-tauri/src/apple_app_attest.m),
[key and enrollment](src-tauri/src/apple.rs),
[connection verification](https://github.com/hellas-ai/hellas/blob/a2fb38b144e10105cf75472810c2b91913374333/crates/client/src/iroh.rs),
[Apple verifier](https://github.com/hellas-ai/hellas/blob/a2fb38b144e10105cf75472810c2b91913374333/crates/attestation/src/apple.rs).
The verifier does not call Apple's fraud-assessment service. Captured evidence
can test verification at capture time; only provisioned hardware can answer a
new hardware challenge.

## Upstream TLS and account limits

Fetch clients disable proxies and redirects and verify TLS hostnames. Public
HTTPS and OpenAI Responses use bundled WebPKI roots, so installing a system CA
does not add an upstream trust anchor. Generic HTTP can explicitly select private
certificate roots; the request's TLS terms must agree with the admitted policy.
The HTTP implementation supports SPKI pin checks, while Gate's resource forms
expose certificate roots. OpenAI Responses uses the public endpoint and bundled
roots; Gate does not claim an additional signed upstream peer attestation.

Code: [TLS verification](https://github.com/hellas-ai/hellas/blob/a2fb38b144e10105cf75472810c2b91913374333/crates/providers/src/http/tls.rs),
[HTTP client](https://github.com/hellas-ai/hellas/blob/a2fb38b144e10105cf75472810c2b91913374333/crates/providers/src/http/clients.rs),
[Responses client](https://github.com/hellas-ai/hellas/blob/a2fb38b144e10105cf75472810c2b91913374333/crates/providers/src/responses_fetch.rs).

Grant budgets, resource limits and signed paid-channel terms restrict admitted
work. They do not promise a reserve in the upstream vendor's account or account
for activity outside Gate. Review price, request limits, credential scope and
vendor billing alongside the provider's identity.

## Checking a release

Inspect the final signed app using `codesign --verify --deep --strict --verbose=2`
and `codesign --display --verbose=4`. Compare the full 64-hex
`CandidateCDHashFull sha256`, the application identifier and signing authority.
Local signature inspection alone does not verify a remote connection. Require
Apple assurance and independently obtained pins in the requester’s trust policy;
approving a policy alone is not successful live verification.

See [signing and provisioning](docs/SIGNING.md) for the platform requirements and
probe procedure. Trust still depends on Apple's root and platform enforcement,
signing-key custody, the reviewed build, client counter storage and selected TLS
anchors and upstream services.
