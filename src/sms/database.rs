#![cfg_attr(not(feature = "http-server"), allow(dead_code))]

use crate::config::DatabaseConfig;
use crate::sms::encryption::SMSEncryption;
use anyhow::{anyhow, Result};
use sms_types::sms::{
    SmsDeliveryReport, SmsMessage
};
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous
};
use sqlx::{Row, SqlitePool};
use std::time::Duration;
use tracing::log::debug;

const SCHEMA_SQL: &str = include_str!("schemas/sqlite.sql");

fn build_pagination_query(
    base_query: &str,
    order_by: &str,
    limit: Option<u64>,
    offset: Option<u64>,
    reverse: bool,
) -> String {
    let order_direction = if reverse { "ASC" } else { "DESC" };
    let mut query = format!("{base_query} ORDER BY {order_by} {order_direction}");

    if let Some(limit_val) = limit {
        query.push_str(&format!(" LIMIT {limit_val}"));
    }

    if let Some(offset_val) = offset {
        query.push_str(&format!(" OFFSET {offset_val}"));
    }

    query
}

pub struct SMSDatabase {
    pool: SqlitePool,
    encryption: SMSEncryption,
}
impl SMSDatabase {
    pub async fn connect(config: DatabaseConfig) -> Result<Self> {
        let connection_options = SqliteConnectOptions::new()
            .filename(&config.database_url)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(30));

        let pool = SqlitePoolOptions::new()
            .max_connections(20)
            .min_connections(5)
            .acquire_timeout(Duration::from_secs(30))
            .idle_timeout(None)
            .max_lifetime(None)
            .test_before_acquire(true)
            .after_connect(|conn, _meta| {
                Box::pin(async move {
                    // Optimise connection.
                    sqlx::query("PRAGMA foreign_keys = ON")
                        .execute(&mut *conn)
                        .await?;
                    sqlx::query("PRAGMA cache_size = -64000")
                        .execute(&mut *conn)
                        .await?; // 64MB Cache
                    sqlx::query("PRAGMA temp_store = memory")
                        .execute(&mut *conn)
                        .await?;
                    Ok(())
                })
            })
            .connect_with(connection_options)
            .await
            .map_err(|e| anyhow!(e))?;

        let db = Self {
            pool,
            encryption: SMSEncryption::new(config.encryption_key),
        };
        db.init_tables().await?;
        Ok(db)
    }

    async fn init_tables(&self) -> Result<()> {
        sqlx::raw_sql(SCHEMA_SQL)
            .execute(&self.pool)
            .await
            .map_err(|e| anyhow!(e))?;

        debug!("SMSDatabase tables initialized successfully!");
        Ok(())
    }

    pub async fn insert_message(&self, message: &SmsMessage, is_final: bool) -> Result<i64> {
        let encrypted_content = self.encryption.encrypt(&message.message_content)?;
        let result = if is_final {
            sqlx::query(
                "INSERT INTO messages (phone_number, message_content, message_reference, is_outgoing, status, completed_at) VALUES (?, ?, ?, ?, ?, unixepoch())"
            )
        } else {
            sqlx::query(
                "INSERT INTO messages (phone_number, message_content, message_reference, is_outgoing, status) VALUES (?, ?, ?, ?, ?)"
            )
        }
            .bind(&message.phone_number)
            .bind(encrypted_content)
            .bind(message.message_reference)
            .bind(message.is_outgoing)
            .bind(message.status)
            .execute(&self.pool)
            .await
            .map_err(|e| anyhow!(e))?;

        Ok(result.last_insert_rowid())
    }

    pub async fn insert_send_failure(
        &self,
        message_id: i64,
        error_message: &String,
    ) -> Result<i64> {
        let result =
            sqlx::query("INSERT INTO send_failures (message_id, error_message) VALUES (?, ?)")
                .bind(message_id)
                .bind(error_message)
                .execute(&self.pool)
                .await
                .map_err(|e| anyhow!(e))?;

        Ok(result.last_insert_rowid())
    }

    pub async fn insert_delivery_report(
        &self,
        message_id: i64,
        status: u8,
        is_final: bool,
    ) -> Result<i64> {
        let result = sqlx::query(
            "INSERT INTO delivery_reports (message_id, status, is_final) VALUES (?, ?, ?)",
        )
        .bind(message_id)
        .bind(status)
        .bind(is_final)
        .execute(&self.pool)
        .await
        .map_err(|e| anyhow!(e))?;

        Ok(result.last_insert_rowid())
    }

    pub async fn get_delivery_report_target_message(
        &self,
        phone_number: &String,
        reference_id: u8,
    ) -> Result<Option<i64>> {
        let result = sqlx::query_scalar(
            "SELECT message_id FROM messages WHERE completed_at IS NULL AND is_outgoing = 1 AND phone_number = ? AND message_reference = ? ORDER BY message_id DESC LIMIT 1"
        )
            .bind(phone_number)
            .bind(reference_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| anyhow!(e))?;

        Ok(result)
    }

    pub async fn update_message_status(
        &self,
        message_id: i64,
        status: u8,
        completed: bool,
    ) -> Result<()> {
        let query = if completed {
            sqlx::query(
                "UPDATE messages SET status = ?, completed_at = unixepoch() WHERE message_id = ?",
            )
        } else {
            sqlx::query("UPDATE messages SET status = ? WHERE message_id = ?")
        };

        query
            .bind(status)
            .bind(message_id)
            .execute(&self.pool)
            .await
            .map_err(|e| anyhow!(e))?;

        Ok(())
    }

    pub async fn update_friendly_name(
        &self,
        phone_number: String,
        friendly_name: Option<String>,
    ) -> Result<()> {
        match friendly_name {
            Some(name) => {
                sqlx::query(
                    "INSERT INTO friendly_names (phone_number, friendly_name) VALUES (?, ?) ON CONFLICT(phone_number) DO UPDATE SET friendly_name = excluded.friendly_name"
                )
                    .bind(&phone_number)
                    .bind(&name)
                    .execute(&self.pool)
                    .await
                    .map_err(|e| anyhow!(e))?;
            }
            None => {
                sqlx::query("DELETE FROM friendly_names WHERE phone_number = ?")
                    .bind(&phone_number)
                    .execute(&self.pool)
                    .await
                    .map_err(|e| anyhow!(e))?;
            }
        }

        Ok(())
    }

    pub async fn get_friendly_name(&self, phone_number: String) -> Result<Option<String>> {
        sqlx::query_scalar("SELECT friendly_name FROM friendly_names WHERE phone_number = ?")
            .bind(phone_number)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| anyhow!(e))
    }

    pub async fn get_latest_numbers(
        &self,
        limit: Option<u64>,
        offset: Option<u64>,
        reverse: bool,
    ) -> Result<Vec<(String, Option<String>)>> {
        let query = build_pagination_query(
            "SELECT m.phone_number, f.friendly_name FROM messages m LEFT JOIN friendly_names f ON f.phone_number = m.phone_number GROUP BY m.phone_number",
            "MAX(m.created_at)",
            limit,
            offset,
            reverse
        );

        let result: Vec<(String, Option<String>)> = sqlx::query_as(&query)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| anyhow!(e))?;

        Ok(result)
    }

    pub async fn get_messages(
        &self,
        phone_number: &str,
        limit: Option<u64>,
        offset: Option<u64>,
        reverse: bool,
    ) -> Result<Vec<SmsMessage>> {
        let query = build_pagination_query(
            "SELECT message_id, phone_number, message_content, message_reference, is_outgoing, status, created_at, completed_at FROM messages WHERE phone_number = ?",
            "created_at",
            limit,
            offset,
            reverse
        );

        let result = sqlx::query(&query)
            .bind(phone_number)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| anyhow!(e))?;

        result
            .into_iter()
            .map(|row| -> Result<SmsMessage> {
                Ok(SmsMessage {
                    message_id: row.get("message_id"),
                    phone_number: row.get("phone_number"),
                    message_content: self
                        .encryption
                        .decrypt(&row.get::<String, _>("message_content"))?,
                    message_reference: row.get("message_reference"),
                    is_outgoing: row.get("is_outgoing"),
                    created_at: row.get("created_at"),
                    completed_at: row.get("completed_at"),
                    status: Some(row.get::<u8, _>("status")),
                })
            })
            .collect::<Result<Vec<_>, _>>()
    }

    pub async fn get_delivery_reports(
        &self,
        message_id: i64,
        limit: Option<u64>,
        offset: Option<u64>,
        reverse: bool,
    ) -> Result<Vec<SmsDeliveryReport>> {
        let query = build_pagination_query(
            "SELECT report_id, message_id, status, is_final, created_at FROM delivery_reports WHERE message_id = ?",
            "created_at",
            limit,
            offset,
            reverse
        );

        sqlx::query_as(&query)
            .bind(message_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| anyhow!(e))
    }
}
