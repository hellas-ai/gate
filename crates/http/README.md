# gate-http

HTTP layer for Gate, providing OpenAI/Anthropic-compatible API routing, middleware, and client functionality. Built on Axum for the server and reqwest for the client.

## Responsibilities

- **API Routes**: OpenAI/Anthropic compatible endpoints (`/v1/chat/completions`, `/v1/messages`)
- **Middleware**: Authentication, tracing, WebAuthn support
- **Request Forwarding**: Routes requests to upstream providers (OpenAI, Anthropic, etc.)
- **Client Library**: Type-safe client for Gate APIs

## Organization

```
src/
├── routes/          # API endpoint handlers
│   ├── inference    # Chat completion & messages endpoints
│   ├── models       # Model listing endpoints
│   └── dashboard    # User management APIs
├── middleware/      # Request processing pipeline
├── forwarding.rs    # Upstream provider integration
├── client/          # Client library (feature = "client")
└── services/        # Business logic (auth, JWT, WebAuthn)
```

## Features

- `default`: Server functionality
- `server`: Full HTTP server with all dependencies
- `client`: Lightweight client library only

## Key Components

### AppState
Central application state containing:
- `StateBackend` implementation
- `UpstreamRegistry` for provider routing
- Plugin manager
- Configuration

### UpstreamRegistry
Manages upstream AI providers:
```rust
registry.register("openai", OpenAIProvider::new(config));
registry.route(request) // Returns appropriate provider
```

### Middleware Pipeline
1. **Tracing**: Request/response logging with correlation IDs
2. **Authentication**: API key validation via `StateBackend`
3. **WebAuthn**: Hardware authentication support

## Usage

### Server
```rust
use gate_http::{server::create_router, AppState};

let state = AppState::new(backend, config);
let app = create_router(state);
// Serve with axum::serve
```

### Client
```rust
use gate_http::client::GateClient;

let client = GateClient::new("https://api.gate.ai", "gk-xxx");
let response = client.chat_completion(request).await?;
```

## Dependencies

Server (heavy):
- `axum`, `tower`, `hyper`: HTTP server stack
- `webauthn-rs`: Hardware auth (native only)
- `jsonwebtoken`: JWT handling

Client (minimal):
- `reqwest`: HTTP client
- `serde_json`: Request/response serialization

## Risks

- **Provider Changes**: Upstream API changes require updates to forwarding logic
- **WASM Limitations**: WebAuthn not available in WASM builds
- **Breaking Changes**: Route changes affect all Gate deployments