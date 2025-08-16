//! WebAuthn backend implementation for SQLx

use chrono::{DateTime, Utc};
use gate_core::{Error, Result};
use serde::{Deserialize, Serialize};
use sqlx::{Database, Executor, FromRow, Pool};

use crate::common::{datetime_to_string, string_to_datetime};

/// WebAuthn credential stored in database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCredential {
    pub credential_id: String,
    pub user_id: String,
    pub public_key: Vec<u8>,
    pub aaguid: Option<Vec<u8>>,
    pub counter: u32,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub device_name: Option<String>,
}

/// Database row type for WebAuthn credentials
#[derive(FromRow)]
pub struct WebAuthnCredentialRow {
    pub credential_id: String,
    pub user_id: String,
    pub public_key: Vec<u8>,
    pub aaguid: Option<Vec<u8>>,
    pub counter: i32,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub device_name: Option<String>,
}

impl From<WebAuthnCredentialRow> for StoredCredential {
    fn from(row: WebAuthnCredentialRow) -> Self {
        StoredCredential {
            credential_id: row.credential_id,
            user_id: row.user_id,
            public_key: row.public_key,
            aaguid: row.aaguid,
            counter: row.counter as u32,
            created_at: string_to_datetime(&row.created_at).unwrap_or_else(|_| chrono::Utc::now()),
            last_used_at: row.last_used_at.and_then(|s| string_to_datetime(&s).ok()),
            device_name: row.device_name,
        }
    }
}

/// SQLx implementation of WebAuthnBackend
pub struct SqlxWebAuthnBackend<DB: Database> {
    pool: Pool<DB>,
}

impl<DB: Database> SqlxWebAuthnBackend<DB> {
    pub fn new(pool: Pool<DB>) -> Self {
        Self { pool }
    }
}

impl<DB> SqlxWebAuthnBackend<DB>
where
    DB: Database,
    for<'c> &'c mut <DB as Database>::Connection: Executor<'c, Database = DB>,
    for<'r> WebAuthnCredentialRow: FromRow<'r, DB::Row>,
    // Required for async_trait with generic parameters
    DB: Send + Sync,
    DB::Connection: Send,
    // Required for parameter binding
    for<'q> String: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
    for<'q> &'q str: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
    for<'q> Option<String>: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
    for<'q> Vec<u8>: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
    for<'q> Option<Vec<u8>>: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
    for<'q> i32: sqlx::Encode<'q, DB> + sqlx::Type<DB>,
    // Required for queries
    for<'q> <DB as Database>::Arguments<'q>: sqlx::IntoArguments<'q, DB>,
{
    pub async fn store_webauthn_credential(&self, credential: &StoredCredential) -> Result<()> {
        sqlx::query(
            "INSERT INTO webauthn_credentials (credential_id, user_id, public_key, aaguid, counter, created_at, last_used_at, device_name) 
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"
        )
        .bind(&credential.credential_id)
        .bind(&credential.user_id)
        .bind(&credential.public_key)
        .bind(&credential.aaguid)
        .bind(credential.counter as i32)
        .bind(datetime_to_string(credential.created_at))
        .bind(credential.last_used_at.map(datetime_to_string))
        .bind(&credential.device_name)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::StateError(format!("Failed to store credential: {e}")))?;

        Ok(())
    }

    pub async fn get_webauthn_credential(
        &self,
        credential_id: &str,
    ) -> Result<Option<StoredCredential>> {
        let row = sqlx::query_as::<_, WebAuthnCredentialRow>(
            "SELECT credential_id, user_id, public_key, aaguid, counter, created_at, last_used_at, device_name 
             FROM webauthn_credentials WHERE credential_id = $1"
        )
        .bind(credential_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::StateError(format!("Failed to get credential: {e}")))?;

        Ok(row.map(Into::into))
    }

    pub async fn list_user_credentials(&self, user_id: &str) -> Result<Vec<StoredCredential>> {
        let rows = sqlx::query_as::<_, WebAuthnCredentialRow>(
            "SELECT credential_id, user_id, public_key, aaguid, counter, created_at, last_used_at, device_name 
             FROM webauthn_credentials WHERE user_id = $1 ORDER BY created_at DESC"
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::StateError(format!("Failed to list user credentials: {e}")))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn list_all_credentials(&self) -> Result<Vec<StoredCredential>> {
        let rows = sqlx::query_as::<_, WebAuthnCredentialRow>(
            "SELECT credential_id, user_id, public_key, aaguid, counter, created_at, last_used_at, device_name 
             FROM webauthn_credentials ORDER BY created_at DESC"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::StateError(format!("Failed to list all credentials: {e}")))?;

        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn update_credential_counter(&self, credential_id: &str, counter: u32) -> Result<()> {
        let now = datetime_to_string(chrono::Utc::now());

        sqlx::query(
            "UPDATE webauthn_credentials SET counter = $1, last_used_at = $2 WHERE credential_id = $3"
        )
        .bind(counter as i32)
        .bind(now)
        .bind(credential_id)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::StateError(format!("Failed to update credential counter: {e}")))?;

        Ok(())
    }

    pub async fn delete_credential(&self, credential_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM webauthn_credentials WHERE credential_id = $1")
            .bind(credential_id)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::StateError(format!("Failed to delete credential: {e}")))?;

        Ok(())
    }
}
