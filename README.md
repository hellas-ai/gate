<div style="text-align: center;" align="center">
<h1>gate</h1>

<!-- [![Crate][crate_img]][crate_link] -->
[![License][license_img]][license_file]
<!-- [![Documentation][docs_img]][docs_link] -->

</div>


_Hellas Gate_ is a user-aligned llm inference server/proxy/router

# in-progress
some of most of this exists, read as a wishlist rather than supported though

- Private local inference via built-in [`catgrad`](https://github.com/hellas-ai/catgrad) engine
- End-to-end encrypted peer-to-peer mesh overlay to aggregate capacity across networked nodes
- Use [LetsEncrypt](https://letsencrypt.org/) to generate a private cert for a public `https://<your-node-id>.private.hellas.ai` endpoint, Hellas will relay it over p2p
- Upstream support for any openai/anthropic-compatible providers, openrouter, vllm, ollama etc 
- Supports [Cursor](https://cursor.ai), [Codex](https://github.com/openai/codex) and [Claude-Code](http://rickroll.com), open-webui, charm, etc via local http server
- API key management, cost-tracking, rate-limiting, quotas, permissions, etc
- Smart router can optimize routing for cost, latency, etc via 'virtual models'
- Capture/Log/Export all requests, responses, metadata through the gateway

# future
- Once [`catgrad`](https://github.com/hellas-ai/catgrad) ZK backend is implemented, we can support _verifying responses_- check request was serviced correctly without quantization, context injection, tampered weights, etc
- Once [`protoproto`](https://github.com/hellas-ai/protoproto) consensus protocol is implemented, we can support settlement and thus create _decentralized_, _trustless and permissionless_ markets for llm inference

[license_file]: https://github.com/hellas-ai/gate/blob/master/LICENSE "Project license"
[license_img]: https://img.shields.io/crates/l/gate.svg?style=for-the-badge "License badge"

