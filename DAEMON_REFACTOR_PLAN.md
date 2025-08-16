# Daemon Actor Refactor - Complete Implementation Plan

## Current State Analysis

### Problems with Current Architecture

1. **Confusing naming**: `Runtime` vs `Daemon` - they're the same thing
2. **Duplicate state management**: 
   - `ServerState` in daemon for HTTP handlers
   - `DaemonState` in GUI for Tauri commands
   - Both representing the same daemon
3. **No clear API boundary**: Direct field access via getters on Runtime
4. **Mixed sync/async patterns**: Some methods sync, some async, no clear reason
5. **Poor error handling**: Using `anyhow` throughout, no proper error types
6. **Scattered permission checks**: In HTTP routes instead of centralized
7. **JSON serialization mess**: Using `json!()` macros instead of proper types
8. **Inconsistent struct definitions**: Different status structs in frontend vs backend

### Current Flow
```
GUI -> DaemonState -> Runtime -> RuntimeInner -> Services
HTTP -> ServerState -> Services
```

## New Architecture Design

### Core Concept
Single daemon actor with clear RPC interface, proper error types, and centralized permission checking.

### New Flow
```
GUI -> Daemon handle -> DaemonActor -> DaemonInner (with permissions)
HTTP -> Daemon handle -> DaemonActor -> DaemonInner (with permissions)
```

## Detailed Implementation Plan

### Phase 1: Define Error Types and Core Structures

#### 1.1 Create daemon error type
```rust
// crates/daemon/src/error.rs

use thiserror::Error;
use gate_core::access::PermissionDenied;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("Permission denied: {0}")]
    PermissionDenied(#[from] PermissionDenied),
    
    #[error("Configuration error: {0}")]
    ConfigError(String),
    
    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),
    
    #[error("Invalid state: {0}")]
    InvalidState(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Database error: {0}")]
    Database(String),
    
    #[error("TLS forward error: {0}")]
    TlsForward(String),
    
    #[error("Channel send error")]
    ChannelSend,
    
    #[error("Channel receive error")]
    ChannelRecv(#[from] oneshot::error::RecvError),
}

// Implement From for mpsc::error::SendError to avoid map_err
impl<T> From<mpsc::error::SendError<T>> for DaemonError {
    fn from(_: mpsc::error::SendError<T>) -> Self {
        DaemonError::ChannelSend
    }
}

pub type Result<T> = std::result::Result<T, DaemonError>;
```

#### 1.2 Define shared status types
```rust
// crates/daemon/src/types.rs

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub running: bool,
    pub listen_address: String,
    pub upstream_count: usize,
    pub user_count: usize,
    pub tlsforward_enabled: bool,
    pub tlsforward_status: TlsForwardStatus,
    pub needs_bootstrap: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TlsForwardStatus {
    Disabled,
    Disconnected,
    Connecting,
    Connected { domain: String },
    Error(String),
}
```

### Phase 2: Implement DaemonInner

#### 2.1 Rename Runtime to DaemonInner
```rust
// crates/daemon/src/daemon/inner.rs

use std::sync::Arc;
use tokio::sync::RwLock;
use crate::error::{DaemonError, Result};
use crate::permissions::{LocalPermissionManager, LocalIdentity};
use crate::types::DaemonStatus;
use gate_core::access::{Action, ObjectIdentity, ObjectKind, ObjectId, TargetNamespace};
use gate_core::StateBackend;

pub struct DaemonInner {
    settings: Arc<RwLock<Settings>>,
    state_backend: Arc<dyn StateBackend>,
    permission_manager: Arc<LocalPermissionManager>,
    auth_service: Arc<AuthService>,
    jwt_service: Arc<JwtService>,
    http_server: Option<HttpServer>,
    tlsforward_service: Option<Arc<TlsForwardService>>,
    bootstrap_token: Option<String>,
    user_count: usize,
}

impl DaemonInner {
    /// Get current daemon status - no permission check needed
    pub async fn status(&self) -> DaemonStatus {
        let settings = self.settings.read().await;
        DaemonStatus {
            running: true,
            listen_address: format!("{}:{}", settings.server.host, settings.server.port),
            upstream_count: settings.upstreams.len(),
            user_count: self.user_count,
            tlsforward_enabled: self.tlsforward_service.is_some(),
            tlsforward_status: self.get_tlsforward_status().await,
            needs_bootstrap: self.user_count == 0,
        }
    }
    
    /// Update daemon configuration - requires Write permission on Config
    pub async fn update_config(
        &mut self,
        identity: &LocalIdentity,
        config: Settings,
    ) -> Result<()> {
        let config_object = ObjectIdentity {
            namespace: TargetNamespace::System,
            kind: ObjectKind::Config,
            id: ObjectId::Singleton,
        };
        
        self.permission_manager
            .check(identity, Action::Write, &config_object)
            .await?;
        
        *self.settings.write().await = config;
        self.reload_services().await?;
        Ok(())
    }
    
    /// Restart daemon - requires Manage permission on Daemon
    pub async fn restart(&mut self, identity: &LocalIdentity) -> Result<()> {
        let daemon_object = ObjectIdentity {
            namespace: TargetNamespace::System,
            kind: ObjectKind::Daemon,
            id: ObjectId::Singleton,
        };
        
        self.permission_manager
            .check(identity, Action::Manage, &daemon_object)
            .await?;
        
        self.shutdown_internal().await?;
        self.start_internal().await?;
        Ok(())
    }
}
```

### Phase 3: Implement Actor Pattern

#### 3.1 Define RPC messages
```rust
// crates/daemon/src/daemon/rpc.rs

use tokio::sync::oneshot;
use crate::error::Result;
use crate::types::DaemonStatus;
use crate::permissions::LocalIdentity;

pub enum DaemonRequest {
    GetStatus {
        reply: oneshot::Sender<DaemonStatus>,
    },
    UpdateConfig {
        identity: LocalIdentity,
        config: Settings,
        reply: oneshot::Sender<Result<()>>,
    },
    Restart {
        identity: LocalIdentity,
        reply: oneshot::Sender<Result<()>>,
    },
    Shutdown {
        identity: LocalIdentity,
        reply: oneshot::Sender<Result<()>>,
    },
}
```

#### 3.2 Implement actor
```rust
// crates/daemon/src/daemon/actor.rs

use tokio::sync::mpsc;
use crate::daemon::inner::DaemonInner;
use crate::daemon::rpc::DaemonRequest;

pub struct DaemonActor {
    inner: DaemonInner,
    rx: mpsc::Receiver<DaemonRequest>,
}

impl DaemonActor {
    pub async fn run(mut self) {
        while let Some(req) = self.rx.recv().await {
            match req {
                DaemonRequest::GetStatus { reply } => {
                    let _ = reply.send(self.inner.status().await);
                }
                DaemonRequest::UpdateConfig { identity, config, reply } => {
                    let result = self.inner.update_config(&identity, config).await;
                    let _ = reply.send(result);
                }
                DaemonRequest::Restart { identity, reply } => {
                    let result = self.inner.restart(&identity).await;
                    let _ = reply.send(result);
                }
                DaemonRequest::Shutdown { identity, reply } => {
                    let result = self.inner.shutdown(&identity).await;
                    let _ = reply.send(result);
                    if result.is_ok() {
                        break;
                    }
                }
            }
        }
    }
}
```

### Phase 4: Implement Public Handle

#### 4.1 Create Daemon handle
```rust
// crates/daemon/src/daemon/mod.rs

use tokio::sync::mpsc;
use tokio::sync::oneshot;
use crate::error::Result;
use crate::permissions::LocalIdentity;

#[derive(Clone)]
pub struct Daemon {
    tx: mpsc::Sender<DaemonRequest>,
    identity: Option<LocalIdentity>,
}

impl Daemon {
    pub fn new(tx: mpsc::Sender<DaemonRequest>) -> Self {
        Self { tx, identity: None }
    }
    
    pub fn with_identity(mut self, identity: LocalIdentity) -> Self {
        self.identity = Some(identity);
        self
    }
    
    pub async fn status(&self) -> Result<DaemonStatus> {
        let (reply, rx) = oneshot::channel();
        self.tx.send(DaemonRequest::GetStatus { reply }).await?;
        Ok(rx.await?)
    }
    
    pub async fn update_config(&self, config: Settings) -> Result<()> {
        let identity = self.identity.clone()
            .ok_or_else(|| DaemonError::InvalidState("No identity set".into()))?;
        
        let (reply, rx) = oneshot::channel();
        self.tx.send(DaemonRequest::UpdateConfig { identity, config, reply }).await?;
        rx.await?
    }
}
```

### Phase 5: Update HTTP Integration

#### 5.1 Update AppState
```rust
// crates/daemon/src/state.rs

pub struct AppState {
    pub daemon: Daemon,
    pub state_backend: Arc<dyn StateBackend>,
}
```

#### 5.2 Update HTTP routes
```rust
// crates/daemon/src/routes/admin.rs

use axum::extract::State;
use axum::response::Json;
use gate_http::error::HttpError;
use gate_http::services::identity::HttpIdentity;

async fn update_config(
    identity: HttpIdentity,
    State(app_state): State<AppState>,
    Json(config): Json<Settings>,
) -> Result<Json<Message>, HttpError> {
    let local_identity = LocalContext::from_http_identity(&identity, &app_state.state_backend).await;
    let local_identity = SubjectIdentity::User {
        id: identity.id,
        context: local_identity,
    };
    
    app_state.daemon
        .clone()
        .with_identity(local_identity)
        .update_config(config)
        .await?;
    
    Ok(Json(Message { msg: "Config updated".into() }))
}
```

Note: HttpError should have `#[from] DaemonError` to avoid map_err

### Phase 6: Update GUI Integration

#### 6.1 Remove DaemonState, use Daemon directly
```rust
// crates/gui/src/main.rs

fn main() {
    tauri::Builder::default()
        .manage(daemon_handle) // Just manage the Daemon handle directly
        .invoke_handler(tauri::generate_handler![
            commands::get_daemon_status,
            // ...
        ])
}
```

#### 6.2 Update Tauri commands
```rust
// crates/gui/src/commands.rs

use tauri::State;
use gate_daemon::Daemon;
use gate_daemon::types::DaemonStatus;

#[tauri::command]
pub async fn get_daemon_status(
    daemon: State<'_, Daemon>,
) -> Result<DaemonStatus, String> {
    daemon.status().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_config(
    daemon: State<'_, Daemon>,
    config: Settings,
) -> Result<String, String> {
    let system_identity = SubjectIdentity::System {
        component: "gui".into(),
        context: LocalContext {
            is_owner: true,
            node_id: "local".into(),
        },
    };
    
    daemon.clone()
        .with_identity(system_identity)
        .update_config(config)
        .await
        .map_err(|e| e.to_string())?;
    
    Ok("Config updated".into())
}
```

Note: In Tauri commands, we DO use map_err because it's application code converting to String for IPC

### Phase 7: Cleanup Frontend

#### 7.1 Use backend types
```rust
// crates/frontend-tauri/src/tauri_api.rs

use serde_wasm_bindgen;
use wasm_bindgen::prelude::*;

pub async fn get_daemon_status() -> Result<DaemonStatus, String> {
    let result = invoke("get_daemon_status", JsValue::UNDEFINED).await?;
    serde_wasm_bindgen::from_value::<DaemonStatus>(result)
        .map_err(|e| e.to_string())
}
```

## Migration Steps

1. **Create new files** (non-breaking):
   - `crates/daemon/src/error.rs`
   - `crates/daemon/src/types.rs`
   - `crates/daemon/src/daemon/` directory structure

2. **Implement DaemonInner** alongside existing Runtime

3. **Add actor pattern** without removing old code

4. **Update one HTTP route** as proof of concept

5. **Update one GUI command** as proof of concept

6. **Gradually migrate** remaining routes and commands

7. **Remove old code**:
   - Delete Runtime files
   - Delete DaemonState from GUI
   - Delete ServerState usage
   - Remove json!() macros

## Benefits

1. **Single source of truth**: One daemon, one API
2. **Type safety**: Proper error types, no anyhow in library code
3. **Clean separation**: Actor for message passing, Inner for business logic
4. **Fine-grained permissions**: Using existing LocalPermissionManager
5. **No duplicate types**: Shared types between frontend and backend
6. **Testable**: Can test DaemonInner directly
7. **Maintainable**: Clear boundaries and responsibilities

## Estimated Work

- Phase 1-2: 2 hours (core structures)
- Phase 3-4: 3 hours (actor implementation)
- Phase 5: 2 hours (HTTP integration)
- Phase 6: 2 hours (GUI integration)
- Phase 7: 1 hour (frontend cleanup)
- Testing: 2 hours

Total: ~12 hours of focused work