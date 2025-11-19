use crate::error::Result;
use crate::models::{Event, EventCreateInput};
use sqlx::{Executor, PgPool, Postgres};
use std::sync::Arc;
use tracing::{instrument, Level};

pub struct EventRepositoryImpl {
    write_pool: Arc<PgPool>,
    read_pool: Arc<PgPool>,
}

impl EventRepositoryImpl {
    /// Create repository with separate read and write pools
    pub fn new(write_pool: Arc<PgPool>, read_pool: Arc<PgPool>) -> Self {
        Self {
            write_pool,
            read_pool,
        }
    }

    /// Create repository with single pool for both read and write operations
    pub fn single_pool(pool: Arc<PgPool>) -> Self {
        Self {
            write_pool: pool.clone(),
            read_pool: pool,
        }
    }

    // Inherent, executor-aware methods (preferred inside a caller-managed transaction)

    #[instrument(
        name = "events.append_with",
        skip_all,
        level = Level::DEBUG,
        fields(aggregate_type = %input.aggregate_type, aggregate_id = %input.aggregate_id, event_type = %input.event_type)
    )]
    pub async fn append_with<'e, E>(&self, exec: E, input: EventCreateInput) -> Result<i64>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let headers = input.headers.unwrap_or_else(|| serde_json::json!({}));
        let version = input.version.unwrap_or(1);

        let row = sqlx::query!(
            r#"
            INSERT INTO events
                (aggregate_type, aggregate_id, event_type, payload, headers, trace_id, span_id, source_service, version, topic)
            VALUES
                ($1,             $2,           $3,         $4,      $5,      $6,       $7,      $8,             $9,      $10)
            RETURNING id
            "#,
            input.aggregate_type,
            input.aggregate_id,
            input.event_type,
            input.payload,
            headers,
            input.trace_id,
            input.span_id,
            input.source_service,
            version,
            input.topic
        )
        .fetch_one(exec)
        .await?;

        Ok(row.id)
    }

    #[instrument(
        name = "events.list_for_aggregate_with",
        skip_all,
        level = Level::DEBUG,
        fields(aggregate_type = %aggregate_type, aggregate_id = %aggregate_id)
    )]
    pub async fn list_for_aggregate_with<'e, E>(
        &self,
        exec: E,
        aggregate_type: &str,
        aggregate_id: &str,
    ) -> Result<Vec<Event>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        // Note: payload and headers are NOT NULL in schema
        let rows = sqlx::query_as!(
            Event,
            r#"
            SELECT
              id,
              occurred_at,
              aggregate_type,
              aggregate_id,
              event_type,
              payload     as "payload!",
              headers     as "headers!",
              trace_id,
              span_id,
              source_service,
              version,
              topic,
              published_at,
              publish_attempts,
              publish_error
            FROM events
            WHERE aggregate_type = $1 AND aggregate_id = $2
            ORDER BY occurred_at, id
            "#,
            aggregate_type,
            aggregate_id
        )
        .fetch_all(exec)
        .await?;

        Ok(rows)
    }

    #[instrument(
        name = "events.list_by_type_with",
        skip_all,
        level = Level::DEBUG,
        fields(event_type = %event_type, limit = limit)
    )]
    pub async fn list_by_type_with<'e, E>(
        &self,
        exec: E,
        event_type: &str,
        limit: i64,
    ) -> Result<Vec<Event>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let rows = sqlx::query_as!(
            Event,
            r#"
            SELECT
              id,
              occurred_at,
              aggregate_type,
              aggregate_id,
              event_type,
              payload     as "payload!",
              headers     as "headers!",
              trace_id,
              span_id,
              source_service,
              version,
              topic,
              published_at,
              publish_attempts,
              publish_error
            FROM events
            WHERE event_type = $1
            ORDER BY occurred_at DESC, id DESC
            LIMIT $2
            "#,
            event_type,
            limit
        )
        .fetch_all(exec)
        .await?;

        Ok(rows)
    }
    #[instrument(
        name = "events.list_unpublished_with",
        skip_all,
        level = Level::DEBUG,
        fields(topic = ?topic, limit = limit)
    )]
    pub async fn list_unpublished_with<'e, E>(
        &self,
        exec: E,
        topic: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Event>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        if let Some(t) = topic {
            let rows = sqlx::query_as!(
                Event,
                r#"
                SELECT
                  id,
                  occurred_at,
                  aggregate_type,
                  aggregate_id,
                  event_type,
                  payload     as "payload!",
                  headers     as "headers!",
                  trace_id,
                  span_id,
                  source_service,
                  version,
                  topic,
                  published_at,
                  publish_attempts,
                  publish_error
                FROM events
                WHERE published_at IS NULL AND topic = $1
                ORDER BY occurred_at, id
                LIMIT $2
                "#,
                t,
                limit
            )
            .fetch_all(exec)
            .await?;
            Ok(rows)
        } else {
            let rows = sqlx::query_as!(
                Event,
                r#"
                SELECT
                  id,
                  occurred_at,
                  aggregate_type,
                  aggregate_id,
                  event_type,
                  payload     as "payload!",
                  headers     as "headers!",
                  trace_id,
                  span_id,
                  source_service,
                  version,
                  topic,
                  published_at,
                  publish_attempts,
                  publish_error
                FROM events
                WHERE published_at IS NULL
                ORDER BY occurred_at, id
                LIMIT $1
                "#,
                limit
            )
            .fetch_all(exec)
            .await?;
            Ok(rows)
        }
    }

    #[instrument(
        name = "events.mark_published_with",
        skip_all,
        level = Level::DEBUG,
        fields(event.id = id)
    )]
    pub async fn mark_published_with<'e, E>(&self, exec: E, id: i64) -> Result<u64>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let res = sqlx::query!(
            r#"
            UPDATE events
            SET published_at = now(),
                publish_error = NULL,
                publish_attempts = publish_attempts + 1
            WHERE id = $1
            "#,
            id
        )
        .execute(exec)
        .await?;
        Ok(res.rows_affected())
    }

    #[instrument(
        name = "events.mark_publish_failed_with",
        skip_all,
        level = Level::DEBUG,
        fields(event.id = id)
    )]
    pub async fn mark_publish_failed_with<'e, E>(
        &self,
        exec: E,
        id: i64,
        error: &str,
    ) -> Result<u64>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let res = sqlx::query!(
            r#"
            UPDATE events
            SET publish_attempts = publish_attempts + 1,
                publish_error = $2
            WHERE id = $1
            "#,
            id,
            error
        )
        .execute(exec)
        .await?;
        Ok(res.rows_affected())
    }

    #[instrument(
        name = "events.append",
        skip(self, input),
        level = Level::DEBUG,
        fields(aggregate_type = %input.aggregate_type, aggregate_id = %input.aggregate_id, event_type = %input.event_type)
    )]
    pub async fn append(&self, input: EventCreateInput) -> Result<i64> {
        self.append_with(&*self.write_pool, input).await
    }

    #[instrument(
        name = "events.list_for_aggregate",
        skip(self),
        level = Level::DEBUG,
        fields(aggregate_type = %aggregate_type, aggregate_id = %aggregate_id)
    )]
    pub async fn list_for_aggregate(
        &self,
        aggregate_type: &str,
        aggregate_id: &str,
    ) -> Result<Vec<Event>> {
        self.list_for_aggregate_with(&*self.read_pool, aggregate_type, aggregate_id)
            .await
    }

    #[instrument(
        name = "events.list_by_type",
        skip(self),
        level = Level::DEBUG,
        fields(event_type = %event_type, limit = limit)
    )]
    pub async fn list_by_type(&self, event_type: &str, limit: i64) -> Result<Vec<Event>> {
        self.list_by_type_with(&*self.read_pool, event_type, limit)
            .await
    }

    #[instrument(
        name = "events.list_unpublished",
        skip(self),
        level = Level::DEBUG,
        fields(topic = ?topic, limit = limit)
    )]
    pub async fn list_unpublished(&self, topic: Option<&str>, limit: i64) -> Result<Vec<Event>> {
        self.list_unpublished_with(&*self.read_pool, topic, limit)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::{PgPool, Postgres, Transaction};

    #[sqlx::test]
    async fn test_append_and_list(pool: PgPool) {
        let repo = EventRepositoryImpl::single_pool(Arc::new(pool.clone()));

        // Append a simple event
        let event_id = repo
            .append(EventCreateInput {
                aggregate_type: "work".into(),
                aggregate_id: "42".into(),
                event_type: "created".into(),
                payload: serde_json::json!({"foo":"bar"}),
                headers: None,
                trace_id: Some("trace-123".into()),
                span_id: Some("span-456".into()),
                source_service: Some("bookapp".into()),
                version: None,
                topic: None,
                published_at: None,
                publish_attempts: None,
                publish_error: None,
            })
            .await
            .expect("append");

        assert!(event_id > 0);

        // List by aggregate
        let by_agg = repo
            .list_for_aggregate("work", "42")
            .await
            .expect("list_for_aggregate");
        assert!(!by_agg.is_empty());
        assert!(by_agg.iter().any(|e| e.id == event_id));
        assert!(by_agg.iter().all(|e| e.payload.is_object()));

        // List by type
        let by_type = repo
            .list_by_type("created", 10)
            .await
            .expect("list_by_type");
        assert!(!by_type.is_empty());
        assert!(by_type.iter().any(|e| e.id == event_id));
    }

    #[sqlx::test]
    async fn test_exec_with_transaction(pool: PgPool) {
        let repo = EventRepositoryImpl::single_pool(Arc::new(pool.clone()));

        let mut tx: Transaction<'_, Postgres> = pool.begin().await.expect("begin");

        // Append within tx
        let id_in_tx = repo
            .append_with(
                tx.as_mut(),
                EventCreateInput {
                    aggregate_type: "edition".into(),
                    aggregate_id: "isbn:9780000000000".into(),
                    event_type: "added".into(),
                    payload: serde_json::json!({"edition": "first"}),
                    headers: None,
                    trace_id: None,
                    span_id: None,
                    source_service: Some("bookapp".into()),
                    version: Some(1),
                    topic: None,
                    published_at: None,
                    publish_attempts: None,
                    publish_error: None,
                },
            )
            .await
            .expect("append_with");

        assert!(id_in_tx > 0);

        // Query inside tx sees the row
        let inside_tx = repo
            .list_for_aggregate_with(tx.as_mut(), "edition", "isbn:9780000000000")
            .await
            .expect("list inside tx");
        assert_eq!(inside_tx.len(), 1);
        assert_eq!(inside_tx[0].id, id_in_tx);

        // Commit the tx so the following non-tx query can see it
        tx.commit().await.expect("commit");

        let outside_tx = repo
            .list_for_aggregate("edition", "isbn:9780000000000")
            .await
            .expect("list after commit");
        assert_eq!(outside_tx.len(), 1);
        assert_eq!(outside_tx[0].id, id_in_tx);
    }
}
