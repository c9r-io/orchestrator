//! Gateway-owned persistence, migrations, and installation fencing.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::json;
use uuid::Uuid;

use crate::crypto::{GatewayCrypto, random_secret};
use crate::domain::{
    ConnectionState, DeliveryProjection, InstallationProjection, NormalizedSlackEvent,
};

const SCHEMA_VERSION: i64 = 1;

/// Decrypted official-app credentials held only for the duration of a provider call.
#[derive(Clone)]
pub struct OfficialAppCredentials {
    /// Slack app ID.
    pub app_id: String,
    /// OAuth client ID.
    pub client_id: String,
    /// OAuth client secret.
    pub client_secret: String,
    /// Slack Events API signing secret.
    pub signing_secret: String,
}

impl std::fmt::Debug for OfficialAppCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OfficialAppCredentials")
            .field("app_id", &self.app_id)
            .field("client_id", &"[REDACTED]")
            .field("client_secret", &"[REDACTED]")
            .field("signing_secret", &"[REDACTED]")
            .finish()
    }
}

/// Input for a new OAuth installation intent.
#[derive(Debug, Clone)]
pub struct NewIntent<'a> {
    /// Local daemon identity.
    pub daemon_id: &'a str,
    /// Project selected by the authenticated daemon admin.
    pub project_id: &'a str,
    /// Stable actor identity; persisted only as a digest.
    pub actor_id: &'a str,
    /// Exact reviewed OAuth redirect URI.
    pub redirect_uri: &'a str,
    /// Exact requested scope names.
    pub requested_scopes: &'a [String],
    /// Intent lifetime.
    pub ttl: Duration,
}

/// OAuth intent material returned once to the creating daemon.
#[derive(Debug, Clone)]
pub struct CreatedIntent {
    /// Stable intent ID.
    pub id: String,
    /// Opaque OAuth state sent only to Slack and the callback.
    pub oauth_state: String,
    /// Secret used by the daemon to poll or cancel this intent.
    pub poll_secret: String,
    /// Expiration timestamp.
    pub expires_at: String,
}

/// Safe OAuth intent status plus a credential revealed only to the authenticated poller.
#[derive(Debug, Clone)]
pub struct IntentStatus {
    /// Intent ID.
    pub id: String,
    /// `pending`, `completed`, `cancelled`, or `failed`.
    pub status: String,
    /// Expiration timestamp.
    pub expires_at: String,
    /// Safe failure code.
    pub error_code: Option<String>,
    /// Installed connection projection.
    pub installation: Option<InstallationProjection>,
    /// Installation-scoped pairing secret, encrypted at rest.
    pub pairing_secret: Option<String>,
}

/// Result returned by Slack OAuth V2 code exchange.
#[derive(Debug, Clone)]
pub struct OAuthInstallation<'a> {
    /// Verified Slack team ID.
    pub team_id: &'a str,
    /// Optional verified Enterprise ID.
    pub enterprise_id: Option<&'a str>,
    /// Granted scope names.
    pub scopes: &'a [String],
    /// Slack bot token.
    pub bot_token: &'a str,
}

/// Authenticated installation secret used by bounded provider calls.
pub struct InstallationCredential {
    /// Safe installation projection.
    pub projection: InstallationProjection,
    /// Decrypted Slack bot token.
    pub bot_token: String,
}

impl std::fmt::Debug for InstallationCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstallationCredential")
            .field("projection", &self.projection)
            .field("bot_token", &"[REDACTED]")
            .finish()
    }
}

/// Thread-safe gateway repository backed by a gateway-private SQLite database.
#[derive(Clone)]
pub struct GatewayStore {
    connection: Arc<Mutex<Connection>>,
    crypto: GatewayCrypto,
}

impl std::fmt::Debug for GatewayStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GatewayStore").finish_non_exhaustive()
    }
}

impl GatewayStore {
    /// Opens the gateway database and applies all forward-only migrations.
    pub fn open(path: &Path, crypto: GatewayCrypto) -> Result<Self> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).context("failed to create gateway data directory")?;
        }
        let connection = Connection::open(path).context("failed to open gateway database")?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .context("failed to set gateway database busy timeout")?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;",
            )
            .context("failed to configure gateway database")?;
        apply_migrations(&connection)?;
        restrict_database_permissions(path)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            crypto,
        })
    }

    /// Produces a stable, non-reversible identity digest for a verified
    /// provider identifier. Raw provider IDs never leave the gateway.
    pub fn identity_digest(&self, purpose: &str, value: &str) -> String {
        self.crypto.digest(purpose, value)
    }

    /// Stores official app credentials encrypted with the gateway master key.
    pub fn put_official_app_credentials(&self, credentials: &OfficialAppCredentials) -> Result<()> {
        let now = now_ts();
        let client_id = self
            .crypto
            .encrypt("official-app:client-id", &credentials.client_id)?;
        let client_secret = self
            .crypto
            .encrypt("official-app:client-secret", &credentials.client_secret)?;
        let signing_secret = self
            .crypto
            .encrypt("official-app:signing-secret", &credentials.signing_secret)?;
        self.connection()?.execute(
            "INSERT INTO official_app_credentials
             (singleton,app_id,client_id_ciphertext,client_secret_ciphertext,signing_secret_ciphertext,updated_at)
             VALUES (1,?1,?2,?3,?4,?5)
             ON CONFLICT(singleton) DO UPDATE SET app_id=excluded.app_id,
             client_id_ciphertext=excluded.client_id_ciphertext,
             client_secret_ciphertext=excluded.client_secret_ciphertext,
             signing_secret_ciphertext=excluded.signing_secret_ciphertext,updated_at=excluded.updated_at",
            params![credentials.app_id, client_id, client_secret, signing_secret, now],
        )?;
        Ok(())
    }

    /// Loads official app credentials without returning them through an API projection.
    pub fn official_app_credentials(&self) -> Result<OfficialAppCredentials> {
        let row = self
            .connection()?
            .query_row(
                "SELECT app_id,client_id_ciphertext,client_secret_ciphertext,signing_secret_ciphertext
                 FROM official_app_credentials WHERE singleton=1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?
            .context("official Slack app credentials are not provisioned")?;
        Ok(OfficialAppCredentials {
            app_id: row.0,
            client_id: self.crypto.decrypt("official-app:client-id", &row.1)?,
            client_secret: self.crypto.decrypt("official-app:client-secret", &row.2)?,
            signing_secret: self.crypto.decrypt("official-app:signing-secret", &row.3)?,
        })
    }

    /// Creates a short-lived OAuth intent and returns its two plaintext one-time secrets.
    pub fn create_intent(&self, input: NewIntent<'_>) -> Result<CreatedIntent> {
        validate_identity(input.daemon_id, "daemon ID")?;
        validate_identity(input.project_id, "project ID")?;
        if input.actor_id.trim().is_empty() {
            bail!("actor ID cannot be empty");
        }
        if input.requested_scopes.is_empty() || input.requested_scopes.len() > 32 {
            bail!("requested scopes must contain 1-32 entries");
        }
        let id = Uuid::new_v4().to_string();
        let oauth_state = random_secret();
        let poll_secret = random_secret();
        let now = Utc::now();
        let expires = now + chrono::Duration::from_std(input.ttl)?;
        let scopes = serde_json::to_string(input.requested_scopes)?;
        self.connection()?.execute(
            "INSERT INTO oauth_intents
             (id,state_digest,poll_digest,daemon_id,project_id,actor_digest,redirect_uri,
              requested_scopes_json,status,expires_at,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'pending',?9,?10,?10)",
            params![
                id,
                self.crypto.digest("oauth-state", &oauth_state),
                self.crypto.digest("intent-poll", &poll_secret),
                input.daemon_id,
                input.project_id,
                self.crypto.digest("actor", input.actor_id),
                input.redirect_uri,
                scopes,
                format_ts(expires),
                format_ts(now),
            ],
        )?;
        Ok(CreatedIntent {
            id,
            oauth_state,
            poll_secret,
            expires_at: format_ts(expires),
        })
    }

    /// Reads an intent only when the installation-specific poll secret matches.
    pub fn intent_status(&self, id: &str, poll_secret: &str) -> Result<IntentStatus> {
        let row = self
            .connection()?
            .query_row(
                "SELECT poll_digest,status,expires_at,error_code,installation_id,pairing_secret_ciphertext
                 FROM oauth_intents WHERE id=?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .optional()?
            .context("intent_not_found")?;
        if !self
            .crypto
            .verify_digest("intent-poll", poll_secret, &row.0)
        {
            bail!("intent_not_found");
        }
        let installation = row
            .4
            .as_deref()
            .map(|installation_id| self.installation_projection(installation_id))
            .transpose()?;
        let pairing_secret = row
            .5
            .as_deref()
            .map(|encrypted| {
                self.crypto
                    .decrypt(&format!("intent:{id}:pairing"), encrypted)
            })
            .transpose()?;
        Ok(IntentStatus {
            id: id.to_string(),
            status: row.1,
            expires_at: row.2,
            error_code: row.3,
            installation,
            pairing_secret,
        })
    }

    /// Cancels one pending intent using its poll secret.
    pub fn cancel_intent(&self, id: &str, poll_secret: &str) -> Result<bool> {
        let status = self.intent_status(id, poll_secret)?;
        if status.status != "pending" {
            return Ok(false);
        }
        let changed = self.connection()?.execute(
            "UPDATE oauth_intents SET status='cancelled',error_code='oauth_cancelled',updated_at=?2
             WHERE id=?1 AND status='pending'",
            params![id, now_ts()],
        )?;
        Ok(changed == 1)
    }

    /// Marks a pending intent as failed using a stable privacy-safe error code.
    pub fn fail_intent_by_state(&self, oauth_state: &str, error_code: &str) -> Result<()> {
        validate_error_code(error_code)?;
        let digest = self.crypto.digest("oauth-state", oauth_state);
        self.connection()?.execute(
            "UPDATE oauth_intents SET status='failed',error_code=?2,consumed_at=?3,updated_at=?3
             WHERE state_digest=?1 AND status='pending' AND expires_at>?3",
            params![digest, error_code, now_ts()],
        )?;
        Ok(())
    }

    /// Returns the exact intent fields required for provider code exchange.
    pub fn pending_intent_by_state(&self, oauth_state: &str) -> Result<PendingIntent> {
        let digest = self.crypto.digest("oauth-state", oauth_state);
        let now = now_ts();
        self.connection()?
            .query_row(
                "SELECT id,daemon_id,project_id,redirect_uri,requested_scopes_json,expires_at
                 FROM oauth_intents WHERE state_digest=?1 AND status='pending' AND expires_at>?2",
                params![digest, now],
                |row| {
                    let scopes_json: String = row.get(4)?;
                    let scopes = serde_json::from_str(&scopes_json).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            scopes_json.len(),
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?;
                    Ok(PendingIntent {
                        id: row.get(0)?,
                        daemon_id: row.get(1)?,
                        project_id: row.get(2)?,
                        redirect_uri: row.get(3)?,
                        requested_scopes: scopes,
                        expires_at: row.get(5)?,
                    })
                },
            )
            .optional()?
            .context("oauth_state_invalid_or_expired")
    }

    /// Completes or reauthorizes one logical installation atomically.
    pub fn complete_intent(
        &self,
        intent: &PendingIntent,
        oauth: OAuthInstallation<'_>,
    ) -> Result<InstallationProjection> {
        let team_digest = self.crypto.digest("slack-team", oauth.team_id);
        let enterprise_digest = oauth
            .enterprise_id
            .map(|value| self.crypto.digest("slack-enterprise", value));
        let scopes_json = serde_json::to_string(oauth.scopes)?;
        let pairing_secret = random_secret();
        let pairing_digest = self.crypto.digest("installation-pairing", &pairing_secret);
        let now = now_ts();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;

        let existing = transaction
            .query_row(
                "SELECT id,owner_daemon_id,owner_project_id,generation,state
                 FROM installations WHERE team_digest=?1",
                [&team_digest],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?;

        let (installation_id, generation) = if let Some(existing) = existing {
            if existing.1 != intent.daemon_id || existing.2 != intent.project_id {
                bail!("installation_owner_conflict");
            }
            if existing.4 == ConnectionState::Disconnected.as_str() {
                bail!("installation_disconnected");
            }
            (existing.0, existing.3 + 1)
        } else {
            (Uuid::new_v4().to_string(), 1)
        };

        let team_id_ciphertext = self.crypto.encrypt(
            &format!("installation:{installation_id}:team"),
            oauth.team_id,
        )?;
        let enterprise_id_ciphertext = oauth
            .enterprise_id
            .map(|value| {
                self.crypto
                    .encrypt(&format!("installation:{installation_id}:enterprise"), value)
            })
            .transpose()?;
        let bot_token_ciphertext = self.crypto.encrypt(
            &format!("installation:{installation_id}:generation:{generation}:bot"),
            oauth.bot_token,
        )?;

        transaction.execute(
            "INSERT INTO installations
             (id,team_digest,enterprise_digest,team_id_ciphertext,enterprise_id_ciphertext,
              owner_daemon_id,owner_project_id,generation,version,state,scopes_json,
              bot_token_ciphertext,pairing_digest,last_acked_cursor,created_at,updated_at,reauthorized_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,1,'active',?9,?10,?11,0,?12,?12,?12)
             ON CONFLICT(id) DO UPDATE SET enterprise_digest=excluded.enterprise_digest,
              team_id_ciphertext=excluded.team_id_ciphertext,
              enterprise_id_ciphertext=excluded.enterprise_id_ciphertext,
              generation=excluded.generation,version=installations.version+1,state='active',
              scopes_json=excluded.scopes_json,bot_token_ciphertext=excluded.bot_token_ciphertext,
              pairing_digest=excluded.pairing_digest,last_error_code=NULL,
              updated_at=excluded.updated_at,reauthorized_at=excluded.reauthorized_at",
            params![
                installation_id,
                team_digest,
                enterprise_digest,
                team_id_ciphertext,
                enterprise_id_ciphertext,
                intent.daemon_id,
                intent.project_id,
                generation,
                scopes_json,
                bot_token_ciphertext,
                pairing_digest,
                now,
            ],
        )?;

        let pairing_ciphertext = self
            .crypto
            .encrypt(&format!("intent:{}:pairing", intent.id), &pairing_secret)?;
        let changed = transaction.execute(
            "UPDATE oauth_intents SET status='completed',installation_id=?2,
             pairing_secret_ciphertext=?3,consumed_at=?4,updated_at=?4
             WHERE id=?1 AND status='pending'",
            params![intent.id, installation_id, pairing_ciphertext, now],
        )?;
        if changed != 1 {
            bail!("oauth_state_already_consumed");
        }
        insert_audit(
            &transaction,
            &installation_id,
            "oauth_completed",
            Some(generation),
            None,
        )?;
        transaction.commit()?;
        drop(connection);
        self.installation_projection(&installation_id)
    }

    /// Lists only installations owned by one daemon and optional project.
    pub fn list_installations(
        &self,
        daemon_id: &str,
        project_id: Option<&str>,
    ) -> Result<Vec<InstallationProjection>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id,team_digest,enterprise_digest,owner_daemon_id,owner_project_id,
             generation,version,state,scopes_json,last_acked_cursor,last_error_code,created_at,updated_at
             FROM installations WHERE owner_daemon_id=?1 AND (?2 IS NULL OR owner_project_id=?2)
             ORDER BY created_at DESC,id DESC",
        )?;
        let rows = statement.query_map(params![daemon_id, project_id], projection_from_row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Authenticates an installation-scoped pairing secret and owner identity.
    pub fn installation_credential(
        &self,
        installation_id: &str,
        daemon_id: &str,
        pairing_secret: &str,
    ) -> Result<InstallationCredential> {
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "SELECT pairing_digest,bot_token_ciphertext,generation,state
                 FROM installations WHERE id=?1 AND owner_daemon_id=?2",
                params![installation_id, daemon_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?
            .context("installation_not_found")?;
        if !self
            .crypto
            .verify_digest("installation-pairing", pairing_secret, &row.0)
        {
            bail!("installation_not_found");
        }
        if row.3 != ConnectionState::Active.as_str() {
            bail!("installation_not_active");
        }
        let bot_token = self.crypto.decrypt(
            &format!("installation:{installation_id}:generation:{}:bot", row.2),
            &row.1,
        )?;
        drop(connection);
        Ok(InstallationCredential {
            projection: self.installation_projection(installation_id)?,
            bot_token,
        })
    }

    /// Authenticates an installation-scoped pairing secret without decrypting
    /// provider credentials. Revoked installations retain this narrow channel
    /// long enough to deliver and acknowledge their lifecycle event.
    pub fn authenticate_pairing(
        &self,
        installation_id: &str,
        daemon_id: &str,
        pairing_secret: &str,
    ) -> Result<InstallationProjection> {
        let connection = self.connection()?;
        let digest = connection
            .query_row(
                "SELECT pairing_digest FROM installations WHERE id=?1 AND owner_daemon_id=?2
                 AND state IN ('active','attention','suspended','revoked')",
                params![installation_id, daemon_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .context("installation_not_found")?;
        if digest.is_empty()
            || !self
                .crypto
                .verify_digest("installation-pairing", pairing_secret, &digest)
        {
            bail!("installation_not_found");
        }
        drop(connection);
        self.installation_projection(installation_id)
    }

    /// Enqueues one normalized provider delivery idempotently.
    pub fn enqueue_delivery(&self, event: &NormalizedSlackEvent) -> Result<bool> {
        let payload = serde_json::to_string(event)?;
        let changed = self.connection()?.execute(
            "INSERT OR IGNORE INTO deliveries
             (id,installation_id,external_event_id,normalized_json,state,created_at,updated_at)
             VALUES (?1,?2,?3,?4,'pending',?5,?5)",
            params![
                Uuid::new_v4().to_string(),
                event.installation_id,
                event.external_event_id,
                payload,
                now_ts()
            ],
        )?;
        Ok(changed == 1)
    }

    /// Claims a bounded delivery batch for one authenticated owner.
    pub fn claim_deliveries(
        &self,
        installation_id: &str,
        daemon_id: &str,
        after_cursor: i64,
        limit: u32,
        lease: Duration,
    ) -> Result<Vec<DeliveryProjection>> {
        if limit == 0 || limit > 100 {
            bail!("delivery limit must be between 1 and 100");
        }
        let now = Utc::now();
        let expires = now + chrono::Duration::from_std(lease)?;
        let now_text = format_ts(now);
        let expires_text = format_ts(expires);
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let owner = transaction
            .query_row(
                "SELECT owner_daemon_id,state FROM installations WHERE id=?1",
                [installation_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
            .context("installation_not_found")?;
        if owner.0 != daemon_id
            || !matches!(
                owner.1.as_str(),
                "active" | "attention" | "suspended" | "revoked"
            )
        {
            bail!("installation_not_found");
        }

        let cursors = {
            let mut statement = transaction.prepare(
                "SELECT cursor FROM deliveries
                 WHERE installation_id=?1 AND cursor>?2 AND state!='acked'
                   AND (state='pending' OR lease_expires_at IS NULL OR lease_expires_at<=?3)
                 ORDER BY cursor ASC LIMIT ?4",
            )?;
            let rows = statement.query_map(
                params![installation_id, after_cursor, now_text, i64::from(limit)],
                |row| row.get::<_, i64>(0),
            )?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for cursor in &cursors {
            transaction.execute(
                "UPDATE deliveries SET state='leased',lease_owner=?2,lease_expires_at=?3,
                 attempt_count=attempt_count+1,updated_at=?4 WHERE cursor=?1",
                params![cursor, daemon_id, expires_text, now_text],
            )?;
        }
        let mut deliveries = Vec::with_capacity(cursors.len());
        for cursor in cursors {
            let (delivery_id, normalized_json) = transaction.query_row(
                "SELECT id,normalized_json FROM deliveries WHERE cursor=?1",
                [cursor],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )?;
            deliveries.push(DeliveryProjection {
                cursor,
                delivery_id,
                event: serde_json::from_str(&normalized_json)?,
                lease_expires_at: expires_text.clone(),
            });
        }
        transaction.commit()?;
        Ok(deliveries)
    }

    /// Acknowledges only deliveries currently leased to the authenticated owner.
    pub fn acknowledge_deliveries(
        &self,
        installation_id: &str,
        daemon_id: &str,
        cursors: &[i64],
    ) -> Result<i64> {
        if cursors.is_empty() || cursors.len() > 100 {
            bail!("ack cursors must contain 1-100 entries");
        }
        let now = now_ts();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let mut max_cursor = 0_i64;
        for cursor in cursors {
            let changed = transaction.execute(
                "UPDATE deliveries SET state='acked',acked_at=?4,updated_at=?4
                 WHERE cursor=?1 AND installation_id=?2 AND lease_owner=?3 AND state='leased'",
                params![cursor, installation_id, daemon_id, now],
            )?;
            if changed != 1 {
                bail!("delivery_ack_fence_failed");
            }
            max_cursor = max_cursor.max(*cursor);
        }
        transaction.execute(
            "UPDATE installations SET last_acked_cursor=MAX(last_acked_cursor,?2),
             updated_at=?3 WHERE id=?1 AND owner_daemon_id=?4",
            params![installation_id, max_cursor, now, daemon_id],
        )?;
        transaction.commit()?;
        Ok(max_cursor)
    }

    /// Resolves the installation from a verified Slack team identity.
    pub fn installation_for_team(&self, team_id: &str) -> Result<InstallationProjection> {
        let digest = self.crypto.digest("slack-team", team_id);
        let connection = self.connection()?;
        let id = connection
            .query_row(
                "SELECT id FROM installations WHERE team_digest=?1 AND state='active'",
                [digest],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .context("unknown_installation")?;
        drop(connection);
        self.installation_projection(&id)
    }

    /// Revokes one installation from a verified Slack lifecycle event.
    pub fn revoke_team(&self, team_id: &str, error_code: &str) -> Result<Option<String>> {
        validate_error_code(error_code)?;
        let digest = self.crypto.digest("slack-team", team_id);
        let connection = self.connection()?;
        let id = connection
            .query_row(
                "SELECT id FROM installations WHERE team_digest=?1",
                [&digest],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(id) = id else {
            return Ok(None);
        };
        let mut connection = connection;
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE deliveries SET state='acked',acked_at=?2,updated_at=?2
             WHERE installation_id=?1 AND state!='acked'",
            params![id, now_ts()],
        )?;
        transaction.execute(
            "UPDATE installations SET state='revoked',version=version+1,last_error_code=?2,
             bot_token_ciphertext='',updated_at=?3 WHERE id=?1",
            params![id, error_code, now_ts()],
        )?;
        insert_audit(
            &transaction,
            &id,
            "installation_revoked",
            None,
            Some(error_code),
        )?;
        transaction.commit()?;
        Ok(Some(id))
    }

    /// Disconnects an authenticated installation and destroys provider and pairing credentials.
    pub fn disconnect_installation(
        &self,
        installation_id: &str,
        daemon_id: &str,
        pairing_secret: &str,
        expected_version: i64,
    ) -> Result<InstallationProjection> {
        self.authenticate_pairing(installation_id, daemon_id, pairing_secret)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE installations SET state='disconnected',version=version+1,
             bot_token_ciphertext='',pairing_digest='',last_error_code=NULL,updated_at=?4
             WHERE id=?1 AND owner_daemon_id=?2 AND version=?3 AND state!='disconnected'",
            params![installation_id, daemon_id, expected_version, now_ts()],
        )?;
        if changed != 1 {
            bail!("installation_version_conflict");
        }
        transaction.execute(
            "UPDATE deliveries SET state='acked',lease_owner=NULL,lease_expires_at=NULL,
             acked_at=?2,updated_at=?2 WHERE installation_id=?1 AND state!='acked'",
            params![installation_id, now_ts()],
        )?;
        insert_audit(
            &transaction,
            installation_id,
            "installation_disconnected",
            None,
            None,
        )?;
        transaction.commit()?;
        drop(connection);
        self.installation_projection(installation_id)
    }

    /// Atomically transfers one installation owner after draining active leases.
    pub fn transfer_installation(
        &self,
        installation_id: &str,
        daemon_id: &str,
        pairing_secret: &str,
        expected_version: i64,
        target_daemon_id: &str,
    ) -> Result<(InstallationProjection, String)> {
        validate_identity(target_daemon_id, "target daemon ID")?;
        self.authenticate_pairing(installation_id, daemon_id, pairing_secret)?;
        if target_daemon_id == daemon_id {
            bail!("installation_owner_unchanged");
        }
        let replacement_pairing = crate::crypto::random_secret();
        let replacement_digest = self
            .crypto
            .digest("installation-pairing", &replacement_pairing);
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE deliveries SET state='pending',lease_owner=NULL,lease_expires_at=NULL,
             updated_at=?2 WHERE installation_id=?1 AND state='leased'",
            params![installation_id, now_ts()],
        )?;
        let changed = transaction.execute(
            "UPDATE installations SET owner_daemon_id=?4,pairing_digest=?5,version=version+1,
             updated_at=?6 WHERE id=?1 AND owner_daemon_id=?2 AND version=?3 AND state='active'",
            params![
                installation_id,
                daemon_id,
                expected_version,
                target_daemon_id,
                replacement_digest,
                now_ts(),
            ],
        )?;
        if changed != 1 {
            bail!("installation_version_conflict");
        }
        insert_audit(
            &transaction,
            installation_id,
            "installation_transferred",
            None,
            None,
        )?;
        transaction.commit()?;
        drop(connection);
        Ok((
            self.installation_projection(installation_id)?,
            replacement_pairing,
        ))
    }

    fn installation_projection(&self, installation_id: &str) -> Result<InstallationProjection> {
        self.connection()?
            .query_row(
                "SELECT id,team_digest,enterprise_digest,owner_daemon_id,owner_project_id,
                 generation,version,state,scopes_json,last_acked_cursor,last_error_code,created_at,updated_at
                 FROM installations WHERE id=?1",
                [installation_id],
                projection_from_row,
            )
            .optional()?
            .context("installation_not_found")
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| anyhow::anyhow!("gateway database lock poisoned"))
    }
}

/// Pending OAuth intent resolved from a valid state digest.
#[derive(Debug, Clone)]
pub struct PendingIntent {
    /// Intent ID.
    pub id: String,
    /// Owning daemon.
    pub daemon_id: String,
    /// Owning project.
    pub project_id: String,
    /// Exact OAuth redirect URI.
    pub redirect_uri: String,
    /// Exact requested scopes.
    pub requested_scopes: Vec<String>,
    /// Expiration timestamp.
    pub expires_at: String,
}

fn apply_migrations(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS gateway_schema_migrations(
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            applied_at TEXT NOT NULL
        );",
    )?;
    let current: i64 = connection.query_row(
        "SELECT COALESCE(MAX(version),0) FROM gateway_schema_migrations",
        [],
        |row| row.get(0),
    )?;
    if current > SCHEMA_VERSION {
        bail!("gateway database schema is newer than this binary");
    }
    if current < 1 {
        connection.execute_batch(
            r#"
            BEGIN IMMEDIATE;
            CREATE TABLE official_app_credentials(
                singleton INTEGER PRIMARY KEY CHECK(singleton=1),
                app_id TEXT NOT NULL,
                client_id_ciphertext TEXT NOT NULL,
                client_secret_ciphertext TEXT NOT NULL,
                signing_secret_ciphertext TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE oauth_intents(
                id TEXT PRIMARY KEY,
                state_digest TEXT NOT NULL UNIQUE,
                poll_digest TEXT NOT NULL,
                daemon_id TEXT NOT NULL,
                project_id TEXT NOT NULL,
                actor_digest TEXT NOT NULL,
                redirect_uri TEXT NOT NULL,
                requested_scopes_json TEXT NOT NULL,
                status TEXT NOT NULL CHECK(status IN ('pending','completed','cancelled','failed')),
                expires_at TEXT NOT NULL,
                consumed_at TEXT,
                installation_id TEXT,
                pairing_secret_ciphertext TEXT,
                error_code TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX idx_oauth_intents_expiry ON oauth_intents(status,expires_at);
            CREATE TABLE installations(
                id TEXT PRIMARY KEY,
                team_digest TEXT NOT NULL UNIQUE,
                enterprise_digest TEXT,
                team_id_ciphertext TEXT NOT NULL,
                enterprise_id_ciphertext TEXT,
                owner_daemon_id TEXT NOT NULL,
                owner_project_id TEXT NOT NULL,
                generation INTEGER NOT NULL CHECK(generation>0),
                version INTEGER NOT NULL CHECK(version>0),
                state TEXT NOT NULL CHECK(state IN ('active','attention','suspended','revoked','disconnected')),
                scopes_json TEXT NOT NULL,
                bot_token_ciphertext TEXT NOT NULL,
                pairing_digest TEXT NOT NULL,
                last_acked_cursor INTEGER NOT NULL DEFAULT 0,
                last_error_code TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                reauthorized_at TEXT
            );
            CREATE INDEX idx_installations_owner ON installations(owner_daemon_id,owner_project_id,state);
            CREATE TABLE deliveries(
                cursor INTEGER PRIMARY KEY AUTOINCREMENT,
                id TEXT NOT NULL UNIQUE,
                installation_id TEXT NOT NULL,
                external_event_id TEXT NOT NULL,
                normalized_json TEXT NOT NULL,
                state TEXT NOT NULL CHECK(state IN ('pending','leased','acked')),
                lease_owner TEXT,
                lease_expires_at TEXT,
                attempt_count INTEGER NOT NULL DEFAULT 0,
                acked_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(installation_id,external_event_id),
                FOREIGN KEY(installation_id) REFERENCES installations(id)
            );
            CREATE INDEX idx_deliveries_claim ON deliveries(installation_id,state,cursor,lease_expires_at);
            CREATE TABLE gateway_audit(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                installation_id TEXT,
                action TEXT NOT NULL,
                generation INTEGER,
                error_code TEXT,
                detail_json TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL
            );
            CREATE INDEX idx_gateway_audit_installation ON gateway_audit(installation_id,created_at);
            INSERT INTO gateway_schema_migrations(version,name,applied_at)
                VALUES (1,'m0001_managed_slack_gateway',strftime('%Y-%m-%dT%H:%M:%SZ','now'));
            COMMIT;
            "#,
        )?;
    }
    Ok(())
}

fn projection_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<InstallationProjection> {
    let scopes_json: String = row.get(8)?;
    let scopes = serde_json::from_str(&scopes_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            scopes_json.len(),
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })?;
    Ok(InstallationProjection {
        id: row.get(0)?,
        team_digest: row.get(1)?,
        enterprise_digest: row.get(2)?,
        owner_daemon_id: row.get(3)?,
        owner_project_id: row.get(4)?,
        generation: row.get(5)?,
        version: row.get(6)?,
        state: row.get(7)?,
        scopes,
        last_acked_cursor: row.get(9)?,
        last_error_code: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn insert_audit(
    transaction: &Transaction<'_>,
    installation_id: &str,
    action: &str,
    generation: Option<i64>,
    error_code: Option<&str>,
) -> Result<()> {
    transaction.execute(
        "INSERT INTO gateway_audit
         (installation_id,action,generation,error_code,detail_json,created_at)
         VALUES (?1,?2,?3,?4,?5,?6)",
        params![
            installation_id,
            action,
            generation,
            error_code,
            json!({}).to_string(),
            now_ts()
        ],
    )?;
    Ok(())
}

fn validate_identity(value: &str, label: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.:".contains(character))
    {
        bail!("{label} must contain 1-128 safe characters");
    }
    Ok(())
}

fn validate_error_code(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 64
        || !value.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
    {
        bail!("error code is invalid");
    }
    Ok(())
}

fn now_ts() -> String {
    format_ts(Utc::now())
}

fn format_ts(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(unix)]
fn restrict_database_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if path.exists() {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .context("failed to restrict gateway database permissions")?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn restrict_database_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    fn setup() -> (tempfile::TempDir, GatewayStore) {
        let temp = tempfile::tempdir().expect("tempdir");
        let store = GatewayStore::open(
            &temp.path().join("gateway.db"),
            GatewayCrypto::from_base64(
                &base64::engine::general_purpose::STANDARD.encode([9_u8; 32]),
            )
            .expect("crypto"),
        )
        .expect("store");
        (temp, store)
    }

    fn create_completed(store: &GatewayStore) -> (CreatedIntent, InstallationProjection) {
        let created = store
            .create_intent(NewIntent {
                daemon_id: "daemon-a",
                project_id: "project-a",
                actor_id: "admin-a",
                redirect_uri: "https://gateway.example/slack/oauth/callback",
                requested_scopes: &["reactions:read".into()],
                ttl: Duration::from_secs(600),
            })
            .expect("intent");
        let pending = store
            .pending_intent_by_state(&created.oauth_state)
            .expect("pending");
        let projection = store
            .complete_intent(
                &pending,
                OAuthInstallation {
                    team_id: "T123",
                    enterprise_id: None,
                    scopes: &["reactions:read".into()],
                    bot_token: "xoxb-test-secret",
                },
            )
            .expect("complete");
        (created, projection)
    }

    #[test]
    fn credentials_and_team_identity_are_not_stored_in_plaintext() {
        let (temp, store) = setup();
        store
            .put_official_app_credentials(&OfficialAppCredentials {
                app_id: "A123".into(),
                client_id: "client-id-secret".into(),
                client_secret: "client-secret-value".into(),
                signing_secret: "signing-secret-value".into(),
            })
            .expect("put credentials");
        let (_, projection) = create_completed(&store);
        assert_eq!(projection.team_digest.len(), 64);
        let bytes = std::fs::read(temp.path().join("gateway.db")).expect("read db");
        let database = String::from_utf8_lossy(&bytes);
        for secret in [
            "client-id-secret",
            "client-secret-value",
            "signing-secret-value",
            "xoxb-test-secret",
            "T123",
        ] {
            assert!(!database.contains(secret), "database leaked secret marker");
        }
    }

    #[test]
    fn intent_state_is_single_use_and_poll_is_secret_bound() {
        let (_temp, store) = setup();
        let (created, projection) = create_completed(&store);
        assert!(store.pending_intent_by_state(&created.oauth_state).is_err());
        assert!(store.intent_status(&created.id, "wrong").is_err());
        let status = store
            .intent_status(&created.id, &created.poll_secret)
            .expect("status");
        assert_eq!(status.status, "completed");
        assert_eq!(status.installation.expect("installation").id, projection.id);
        assert!(status.pairing_secret.is_some());
    }

    #[test]
    fn repeated_oauth_reauthorizes_one_logical_installation_and_fences_old_pairing() {
        let (_temp, store) = setup();
        let (first_intent, first) = create_completed(&store);
        let first_secret = store
            .intent_status(&first_intent.id, &first_intent.poll_secret)
            .expect("first status")
            .pairing_secret
            .expect("first secret");

        let second = store
            .create_intent(NewIntent {
                daemon_id: "daemon-a",
                project_id: "project-a",
                actor_id: "admin-a",
                redirect_uri: "https://gateway.example/slack/oauth/callback",
                requested_scopes: &["reactions:read".into()],
                ttl: Duration::from_secs(600),
            })
            .expect("second intent");
        let pending = store
            .pending_intent_by_state(&second.oauth_state)
            .expect("pending");
        let reauthorized = store
            .complete_intent(
                &pending,
                OAuthInstallation {
                    team_id: "T123",
                    enterprise_id: None,
                    scopes: &["reactions:read".into()],
                    bot_token: "xoxb-generation-two",
                },
            )
            .expect("reauthorize");
        assert_eq!(reauthorized.id, first.id);
        assert_eq!(reauthorized.generation, 2);
        assert!(
            store
                .installation_credential(&first.id, "daemon-a", &first_secret)
                .is_err()
        );
    }

    #[test]
    fn owner_conflict_fails_closed() {
        let (_temp, store) = setup();
        create_completed(&store);
        let conflicting = store
            .create_intent(NewIntent {
                daemon_id: "daemon-b",
                project_id: "project-b",
                actor_id: "admin-b",
                redirect_uri: "https://gateway.example/slack/oauth/callback",
                requested_scopes: &["reactions:read".into()],
                ttl: Duration::from_secs(600),
            })
            .expect("intent");
        let pending = store
            .pending_intent_by_state(&conflicting.oauth_state)
            .expect("pending");
        let error = store
            .complete_intent(
                &pending,
                OAuthInstallation {
                    team_id: "T123",
                    enterprise_id: None,
                    scopes: &["reactions:read".into()],
                    bot_token: "xoxb-other",
                },
            )
            .expect_err("owner conflict");
        assert!(error.to_string().contains("installation_owner_conflict"));
    }

    #[test]
    fn transfer_rotates_pairing_and_moves_exactly_one_owner() {
        let (_temp, store) = setup();
        let (intent, installation) = create_completed(&store);
        let first_pairing = store
            .intent_status(&intent.id, &intent.poll_secret)
            .expect("status")
            .pairing_secret
            .expect("pairing");
        let (transferred, replacement) = store
            .transfer_installation(
                &installation.id,
                "daemon-a",
                &first_pairing,
                installation.version,
                "daemon-b",
            )
            .expect("transfer");
        assert_eq!(transferred.owner_daemon_id, "daemon-b");
        assert_eq!(transferred.version, installation.version + 1);
        assert!(
            store
                .installation_credential(&installation.id, "daemon-a", &first_pairing)
                .is_err()
        );
        assert!(
            store
                .installation_credential(&installation.id, "daemon-b", &replacement)
                .is_ok()
        );
        assert!(
            store
                .transfer_installation(
                    &installation.id,
                    "daemon-a",
                    &first_pairing,
                    installation.version,
                    "daemon-c",
                )
                .is_err()
        );
    }

    #[test]
    fn delivery_claim_and_ack_are_owner_fenced_and_idempotent() {
        let (_temp, store) = setup();
        let (intent, installation) = create_completed(&store);
        let pairing = store
            .intent_status(&intent.id, &intent.poll_secret)
            .expect("status")
            .pairing_secret
            .expect("pairing");
        let event = NormalizedSlackEvent {
            external_event_id: "Ev1".into(),
            event_type: "reaction_added".into(),
            installation_id: installation.id.clone(),
            external_actor_id: Some("U1".into()),
            reaction: Some("agent-review".into()),
            channel_id: Some("C1".into()),
            message_ts: Some("1.2".into()),
            event_ts: "1.3".into(),
            team_digest: installation.team_digest.clone(),
            enterprise_digest: None,
        };
        assert!(store.enqueue_delivery(&event).expect("enqueue"));
        assert!(!store.enqueue_delivery(&event).expect("dedupe"));
        assert!(
            store
                .installation_credential(&installation.id, "daemon-a", &pairing)
                .is_ok()
        );
        let claimed = store
            .claim_deliveries(&installation.id, "daemon-a", 0, 10, Duration::from_secs(30))
            .expect("claim");
        assert_eq!(claimed.len(), 1);
        assert!(
            store
                .acknowledge_deliveries(&installation.id, "daemon-b", &[claimed[0].cursor])
                .is_err()
        );
        assert_eq!(
            store
                .acknowledge_deliveries(&installation.id, "daemon-a", &[claimed[0].cursor])
                .expect("ack"),
            claimed[0].cursor
        );
        let after_ack = store
            .authenticate_pairing(&installation.id, "daemon-a", &pairing)
            .expect("projection after ack");
        assert_eq!(after_ack.version, installation.version);
        let disconnected = store
            .disconnect_installation(&installation.id, "daemon-a", &pairing, installation.version)
            .expect("disconnect after delivery");
        assert_eq!(disconnected.state, "disconnected");
    }
}
