use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::sleep;
use tracing::{debug, error, info, instrument, warn};

use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::Message;

use bookapp_dal::repository::EventRepositoryImpl;
use bookapp_dal::{Event, PgPool};

/// Configuration for the outbox publisher worker
#[derive(Clone, Debug)]
pub struct OutboxPublisherConfig {
    pub default_topic: Option<String>,
    pub batch_size: i64,
    pub poll_interval: Duration,
}

/// Periodically scans the events outbox table for unpublished events and publishes them to Kafka.
/// Marks events as published or failed accordingly.
pub async fn start_outbox_publisher(
    pool: Arc<PgPool>,
    producer: FutureProducer,
    config: OutboxPublisherConfig,
    shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let repo = EventRepositoryImpl::single_pool(pool.clone());

    info!(
        "Starting outbox publisher with interval {:?}",
        config.poll_interval
    );
    loop {
        if *shutdown.borrow() {
            info!("Outbox publisher received shutdown signal");
            break;
        }

        // Create a new root span for each polling cycle
        let cycle_span = tracing::info_span!(
            "outbox_publish_cycle",
            batch_size = %config.batch_size,
            otel.kind = "internal"
        );

        let result = cycle_span
            .in_scope(|| async {
                publish_unpublished_events(&repo, pool.as_ref(), &producer, &config).await
            })
            .await;

        match result {
            Ok(count) => {
                if count == 0 {
                    // Nothing to do, sleep the full interval
                    sleep(config.poll_interval).await;
                } else {
                    // If we published something, yield briefly before next scan
                    tokio::task::yield_now().await;
                }
            }
            Err(err) => {
                error!(error = %err, "Outbox publish cycle failed");
                sleep(config.poll_interval).await;
            }
        }
    }

    info!("Outbox publisher stopped");
    Ok(())
}

#[instrument(skip_all)]
async fn publish_unpublished_events(
    repo: &EventRepositoryImpl,
    pool: &PgPool,
    producer: &FutureProducer,
    config: &OutboxPublisherConfig,
) -> Result<usize> {
    // We don't filter by topic here to allow per-event topic overrides in DB (event.topic)
    let events = repo
        .list_unpublished_with(pool, None, config.batch_size)
        .await?;

    if events.is_empty() {
        debug!("No unpublished events found");
        return Ok(0);
    }

    debug!(count = events.len(), "Found unpublished events");
    let mut published_count = 0usize;

    for ev in events {
        // Determine topic: event.topic overrides default
        let topic = match (&ev.topic, &config.default_topic) {
            (Some(t), _) if !t.is_empty() => t.clone(),
            (None, Some(t)) if !t.is_empty() => t.clone(),
            _ => {
                warn!(event_id = ev.id, "No topic configured for event; skipping");
                // Mark as failed so we don't spin forever; alternatively leave it for manual intervention
                let _ = repo
                    .mark_publish_failed_with(pool, ev.id, "no topic configured")
                    .await;
                continue;
            }
        };

        match publish_event(producer, &topic, &ev).await {
            Ok(()) => {
                let _ = repo.mark_published_with(pool, ev.id).await;
                published_count += 1;
            }
            Err(e) => {
                error!(event_id = ev.id, error = %e, "Failed to publish outbox event");
                let _ = repo
                    .mark_publish_failed_with(pool, ev.id, &format!("{e:#}"))
                    .await;
            }
        }
    }

    Ok(published_count)
}

#[instrument(skip_all, fields(topic = %topic, event.id = event.id))]
async fn publish_event(producer: &FutureProducer, topic: &str, event: &Event) -> Result<()> {
    // Use aggregate_id as key for ordering/partition affinity where possible
    let key = event.aggregate_id.as_str();

    // We serialize the entire event payload. Headers can be extended to include trace context.
    let payload = serde_json::to_vec(&event.payload)?;

    // Simple record without headers for now - can add headers back later
    let rec = FutureRecord::to(topic).key(key).payload(&payload);

    // Send with a timeout; backpressure via await
    let delivery_timeout = Duration::from_secs(10);
    match producer.send(rec, delivery_timeout).await {
        Ok(delivery) => {
            let (_partition, _offset) = delivery;
            debug!(?delivery, "Kafka delivery succeeded");
            Ok(())
        }
        Err((e, _msg)) => Err(e.into()),
    }
}
use bookapp_dal::BookRepositoryImpl;
use opentelemetry::global;
use opentelemetry::propagation::Extractor;
use opentelemetry::trace::TraceContextExt;
use rdkafka::{
    config::ClientConfig,
    consumer::{CommitMode, Consumer, StreamConsumer},
    message::Headers,
};
use serde::{Deserialize, Serialize};
use tracing_opentelemetry::OpenTelemetrySpanExt;

#[derive(Serialize, Deserialize, Debug)]
pub struct BookIngestionMessage {
    pub book_id: i32,
    // other fields if necessary
}

struct HeaderExtractor<'a> {
    headers: Option<&'a rdkafka::message::BorrowedHeaders>,
}

impl Extractor for HeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.headers.and_then(|headers| {
            headers.iter().find_map(|header| {
                if header.key.eq_ignore_ascii_case(key) {
                    header.value.and_then(|v| std::str::from_utf8(v).ok())
                } else {
                    None
                }
            })
        })
    }

    fn keys(&self) -> Vec<&str> {
        self.headers
            .map_or_else(Vec::new, |headers| headers.iter().map(|h| h.key).collect())
    }
}

#[tracing::instrument(skip(book_repository), fields(book_id))]
async fn background_process_new_book(
    book_id: i32,
    book_repository: Arc<BookRepositoryImpl>,
) -> Result<()> {
    info!(
        book_id = book_id,
        "Starting background processing for new book"
    );

    // Refresh the search materialized view to include the new book
    // This ensures the book is immediately available in full-text search
    if let Err(e) = refresh_search_materialized_view_for_new_book(book_repository.clone()).await {
        error!(
            book_id = book_id,
            error = %e,
            "Failed to refresh search view for new book"
        );
        // Don't fail the entire process if view refresh fails
    } else {
        info!(
            book_id = book_id,
            "Successfully refreshed search view for new book"
        );
    }

    // Additional background processing could include:
    // - Fetch additional metadata from external APIs
    // - Perform content analysis
    // - Generate thumbnails or previews
    // - Send notifications to subscribers

    info!(
        book_id = book_id,
        "Completed background processing for new book"
    );

    Ok(())
}

/// Refreshes the book search materialized view after new book creation
/// This ensures new books are immediately available in full-text search results
#[tracing::instrument(
    skip(book_repository),
    fields(
        operation = "refresh_materialized_view",
        view.name = "book_search_view",
        view.refresh_type = "concurrent",
        view.refresh_duration_ms,
        view.rows_affected,
        maintenance.type = "event_driven"
    )
)]
async fn refresh_search_materialized_view_for_new_book(
    book_repository: Arc<BookRepositoryImpl>,
) -> Result<()> {
    let start_time = std::time::Instant::now();

    info!(
        view.name = "book_search_view",
        operation = "refresh_materialized_view",
        "Starting materialized view refresh for new book"
    );

    // Access the write pool directly for this database maintenance operation
    let pool = book_repository.write_pool();

    // Refresh the materialized view concurrently (non-blocking for reads)
    let result = sqlx::query("REFRESH MATERIALIZED VIEW CONCURRENTLY book_search_view")
        .execute(pool.as_ref())
        .await?;

    let refresh_duration = start_time.elapsed();
    let rows_affected = result.rows_affected();

    // Record span attributes
    tracing::Span::current().record(
        "view.refresh_duration_ms",
        refresh_duration.as_millis() as u64,
    );
    tracing::Span::current().record("view.rows_affected", rows_affected);

    info!(
        view.name = "book_search_view",
        view.refresh_duration_ms = refresh_duration.as_millis(),
        view.rows_affected = rows_affected,
        operation = "refresh_materialized_view",
        "Materialized view refresh completed for new book"
    );

    Ok(())
}

pub fn create_consumer() -> Result<StreamConsumer> {
    let kafka_broker_url =
        std::env::var("KAFKA_BROKER_URL").unwrap_or_else(|_| "kafka:9092".to_string());
    let kafka_group_id =
        std::env::var("KAFKA_GROUP_ID").unwrap_or_else(|_| "backend_consumer_group".to_string());

    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", &kafka_broker_url)
        .set("group.id", &kafka_group_id)
        .set("auto.offset.reset", "earliest")
        .set("session.timeout.ms", "6000")
        .set("enable.auto.commit", "false")
        .create()
        .map_err(|e| anyhow::anyhow!("Consumer creation failed: {:?}", e))?;

    Ok(consumer)
}

pub async fn run_consumer(book_repository: Arc<BookRepositoryImpl>) -> Result<()> {
    let consumer = create_consumer()?;

    consumer.subscribe(&["book_ingestion"])?;

    info!("Backend Kafka consumer started, waiting for messages...");

    loop {
        match consumer.recv().await {
            Err(e) => error!("Kafka error: {}", e),
            Ok(m) => {
                let payload = match m.payload_view::<str>() {
                    None => "",
                    Some(Ok(s)) => s,
                    Some(Err(e)) => {
                        error!(
                            error = format!("{e:#}"),
                            "Error while deserializing payload"
                        );
                        continue;
                    }
                };

                // Create a new root span for this message processing
                let span = tracing::info_span!(
                    "book_ingestion_processing",
                    "otel.kind" = "Consumer",
                    "messaging.system" = "kafka",
                    "messaging.destination" = "book_ingestion"
                );

                // Extract tracing context from headers
                let headers = m.headers();
                let extractor = HeaderExtractor { headers };

                // Extract the parent OpenTelemetry context
                let parent_cx =
                    global::get_text_map_propagator(|propagator| propagator.extract(&extractor));

                // Extract the linked span context from the otel context
                let linked_span_context = parent_cx.span().span_context().clone();
                tracing::debug!(
                    trace_id = %linked_span_context.trace_id(),
                    span_id = %linked_span_context.span_id(),
                    "Extracting context from linked span"
                );

                // Link the extracted span context to our current root span
                // This creates a linked span rather than a parent-child relationship
                // which is appropriate for async message processing
                let link_attributes = vec![
                    opentelemetry::KeyValue::new("link.type", "follows_from"),
                    opentelemetry::KeyValue::new("messaging.operation", "process"),
                ];
                span.add_link_with_attributes(linked_span_context, link_attributes);

                let processing_result = span
                    .in_scope(|| async {
                        // Deserialize and process the message
                        if let Ok(book_message) =
                            serde_json::from_str::<BookIngestionMessage>(payload)
                        {
                            info!(
                                book_id = book_message.book_id,
                                partition = m.partition(),
                                offset = m.offset(),
                                "Processing book ingestion message in backend"
                            );

                            // Process the message with the repository
                            if let Err(e) = background_process_new_book(
                                book_message.book_id,
                                book_repository.clone(),
                            )
                            .await
                            {
                                error!(
                                    book_id = book_message.book_id,
                                    error = %e,
                                    "Failed to process book ingestion message"
                                );
                                return Err(e);
                            }
                        } else {
                            error!("Failed to deserialize message payload");
                            return Err(anyhow::anyhow!("Failed to deserialize message payload"));
                        }
                        Ok(())
                    })
                    .await;

                // Commit the message offset only if processing succeeded
                match processing_result {
                    Ok(()) => {
                        if let Err(e) = consumer.commit_message(&m, CommitMode::Async) {
                            error!("Failed to commit message offset: {:?}", e);
                        }
                    }
                    Err(e) => {
                        error!("Message processing failed, not committing offset: {:?}", e);
                        // In a production system, you might want to:
                        // - Send to a dead letter queue
                        // - Retry with exponential backoff
                        // - Alert monitoring systems
                    }
                }
            }
        }
    }
}
