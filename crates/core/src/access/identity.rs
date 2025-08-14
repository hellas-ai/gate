use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{self, Display};
use std::ops::Deref;
use thiserror::Error;

/// Identity of the subject performing an action
#[derive(Debug, Clone)]
pub struct SubjectIdentity<C: IdentityContext> {
    pub id: String,
    pub source: String,
    pub context: C,
}

impl<C: IdentityContext> SubjectIdentity<C> {
    pub fn new(id: impl Into<String>, source: impl Into<String>, context: C) -> Self {
        Self {
            id: id.into(),
            source: source.into(),
            context,
        }
    }
}

/// Context information for an identity
pub trait IdentityContext: Send + Sync + Clone + std::fmt::Debug {
    /// Serialize context to key-value pairs
    fn to_attributes(&self) -> HashMap<String, String>;

    /// Get a specific attribute value
    fn get(&self, key: &str) -> Option<&str>;
}

/// Namespace for target objects
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TargetNamespace {
    System,
    Local,
    Organization(String),
    Node(String),
}

impl Display for TargetNamespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::System => write!(f, "system"),
            Self::Local => write!(f, "local"),
            Self::Organization(org) => write!(f, "org:{org}"),
            Self::Node(node) => write!(f, "node:{node}"),
        }
    }
}

/// Type of object being accessed
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObjectKind {
    Model,
    Provider,
    User,
    Users,
    Config,
    Billing,
    System,
    Quota,
}

impl Display for ObjectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Unique identifier for an object
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObjectId(String);

impl ObjectId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl Deref for ObjectId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<&str> for ObjectId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for ObjectId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Identity of the object being accessed
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObjectIdentity {
    pub namespace: TargetNamespace,
    pub kind: ObjectKind,
    pub id: ObjectId,
}

impl ObjectIdentity {
    pub fn local_model(model_id: impl Into<String>) -> Self {
        Self {
            namespace: TargetNamespace::Local,
            kind: ObjectKind::Model,
            id: ObjectId::new(model_id),
        }
    }

    pub fn org_model(org: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self {
            namespace: TargetNamespace::Organization(org.into()),
            kind: ObjectKind::Model,
            id: ObjectId::new(model_id),
        }
    }

    pub fn local_provider(provider_id: impl Into<String>) -> Self {
        Self {
            namespace: TargetNamespace::Local,
            kind: ObjectKind::Provider,
            id: ObjectId::new(provider_id),
        }
    }

    pub fn quota(user_id: impl Into<String>) -> Self {
        Self {
            namespace: TargetNamespace::Local,
            kind: ObjectKind::Quota,
            id: ObjectId::new(user_id),
        }
    }

    pub fn wildcard() -> Self {
        Self {
            namespace: TargetNamespace::Local,
            kind: ObjectKind::System,
            id: ObjectId::new("*"),
        }
    }

    /// Parse an object identity from a string format "namespace/kind/id"
    pub fn from_string(s: &str) -> Result<Self, String> {
        let parts: Vec<&str> = s.split('/').collect();
        if parts.len() != 3 {
            return Err(format!(
                "Invalid object format: expected namespace/kind/id, got {s}"
            ));
        }

        let namespace = match parts[0] {
            "system" => TargetNamespace::System,
            "local" => TargetNamespace::Local,
            ns if ns.starts_with("org:") => {
                TargetNamespace::Organization(ns.strip_prefix("org:").unwrap().to_string())
            }
            ns if ns.starts_with("node:") => {
                TargetNamespace::Node(ns.strip_prefix("node:").unwrap().to_string())
            }
            _ => return Err(format!("Invalid namespace: {}", parts[0])),
        };

        let kind = match parts[1] {
            "Model" => ObjectKind::Model,
            "Provider" => ObjectKind::Provider,
            "User" => ObjectKind::User,
            "Users" => ObjectKind::Users,
            "Config" => ObjectKind::Config,
            "Billing" => ObjectKind::Billing,
            "System" => ObjectKind::System,
            "Quota" => ObjectKind::Quota,
            _ => return Err(format!("Invalid object kind: {}", parts[1])),
        };

        Ok(Self {
            namespace,
            kind,
            id: ObjectId::new(parts[2]),
        })
    }
}

impl Display for ObjectIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}/{}", self.namespace, self.kind, self.id)
    }
}

#[derive(Debug, Error)]
pub enum AuthenticationError {
    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("Authentication failed: {0}")]
    Failed(String),

    #[error("Token expired")]
    TokenExpired,

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Establishes identity from credentials
#[async_trait]
pub trait Authentication: Send + Sync {
    type Context: IdentityContext;

    async fn authenticate(
        &self,
        credentials: &serde_json::Value,
    ) -> Result<SubjectIdentity<Self::Context>, AuthenticationError>;
}
