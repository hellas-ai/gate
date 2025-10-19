//! Routing plan definition
use super::connector::RequestContext;
use super::protocols::ProtocolConversion;
use super::types::RetryConfig;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Executable routing plan
#[derive(Debug, Clone)]
pub struct RoutingPlan {
    pub id: uuid::Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub context: RequestContext,
    pub primary_route: Route,
    pub fallback_routes: Vec<Route>,
    pub estimated_cost: Option<Decimal>,
}

/// A single route in the plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Route {
    pub connector_id: String,
    pub protocol_conversion: Option<ProtocolConversion>,
    pub timeout: Duration,
    pub retry_config: RetryConfig,
}

impl RoutingPlan {
    /// Create a new routing plan
    pub fn new(context: RequestContext, primary_route: Route, fallback_routes: Vec<Route>) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            created_at: chrono::Utc::now(),
            context,
            primary_route,
            fallback_routes,
            estimated_cost: None,
        }
    }

    /// Set estimated cost
    pub fn with_estimated_cost(mut self, cost: Decimal) -> Self {
        self.estimated_cost = Some(cost);
        self
    }
}
