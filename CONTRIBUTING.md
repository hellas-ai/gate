# Contributing

Enter the pinned environment with `nix develop`, then run `make check` before
submitting changes. The flake routes Rust build output to `/tmp` and supplies
the frontend tools directly; do not run `npm install` or create caches in this
checkout.

Gate should stay a thin desktop host. Protocol types, verification, transports,
and reusable client/provider/gateway behavior belong in the sibling `hellas`
workspace. Platform app integration, lifecycle policy, local persistence, and
the Tauri UI belong here.

Do not add an internal HTTP control plane, a companion daemon, a second protocol
implementation, or a package-publication step. Keep Tauri invocations in
`ui/src/api.ts`, and keep the corresponding DTOs derived from Rust.
