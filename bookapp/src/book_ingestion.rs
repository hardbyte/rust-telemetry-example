use anyhow::Result;
use opentelemetry::propagation::Extractor;
use opentelemetry::trace::{Link, Span, SpanKind, TraceContextExt, Tracer};
use opentelemetry::{global, propagation::Injector, Context as OtelContext};
use rdkafka::message::Header;
use rdkafka::util::Timeout;
use rdkafka::{
    config::ClientConfig,
    consumer::{CommitMode, Consumer, StreamConsumer},
    message::{Headers, Message, OwnedHeaders},
    producer::{FutureProducer, FutureRecord},
};
use serde::{Deserialize, Serialize};
use tracing::{error, info};
use tracing_opentelemetry::OpenTelemetrySpanExt;

#[derive(Serialize, Deserialize, Debug)]
pub struct BookIngestionMessage {
    pub(crate) book_id: i32,
    // other fields if necessary
}

struct VecInjector {
    headers: Vec<(String, String)>,
}

impl VecInjector {
    fn new() -> Self {
        VecInjector {
            headers: Vec::new(),
        }
    }

    fn into_owned_headers(self) -> OwnedHeaders {
        let mut headers = OwnedHeaders::new();
        for (key, value) in self.headers {
            headers = headers.insert(Header {
                key: &key,
                value: Some(&value),
            });
        }
        headers
    }
}

impl Injector for VecInjector {
    fn set(&mut self, key: &str, value: String) {
        self.headers.push((key.to_owned(), value));
    }
}

struct HeaderExtractor<'a> {
    headers: Option<&'a rdkafka::message::BorrowedHeaders>,
}

impl<'a> Extractor for HeaderExtractor<'a> {
    fn get(&self, key: &str) -> Option<&str> {
        self.headers.and_then(|headers| {
            headers.iter().find_map(|header| {
                if header.key.eq_ignore_ascii_case(key) {
                    std::str::from_utf8(header.value.unwrap()).ok()
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

#[tracing::instrument]
fn background_process_new_book(book_id: i32) {
    // This function simulates a background process that processes new books
    // In a real-world scenario, this function would be more complex and
    // perform various tasks such as data validation, enrichment, and transformation.
    // For simplicity, we will just print a message here.
    info!(
        book_id = book_id,
        "Starting processing new book in the background"
    );
    // Sleep for a short time to simulate processing
    std::thread::sleep(std::time::Duration::from_millis(5));
    info!(
        book_id = book_id,
        "Completed processing new book in the background"
    );
}

pub fn create_producer() -> Result<FutureProducer> {
    let kafka_broker_url =
        std::env::var("KAFKA_BROKER_URL").unwrap_or_else(|_| "kafka:9092".to_string());

    let producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", &kafka_broker_url)
        .set("message.timeout.ms", "5000")
        .set("retries", "10")
        .set("retry.backoff.ms", "1000")
        .create()
        .map_err(|e| anyhow::anyhow!("Producer creation error: {:?}", e))?;

    Ok(producer)
}

pub async fn send_book_ingestion_message(
    producer: &FutureProducer,
    book_message: &BookIngestionMessage,
    otel_context: &OtelContext,
) -> Result<()> {
    let payload = serde_json::to_string(&book_message)?;

    // Collect OpenTelemetry headers
    let mut injector = VecInjector::new();

    // Inject the tracing context into the headers
    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(otel_context, &mut injector);
    });

    // Create Kafka record with headers
    let key = format!("key-{}", &book_message.book_id.to_string());
    let record = FutureRecord::to("book_ingestion")
        .key(&key)
        .payload(&payload)
        .headers(injector.into_owned_headers());

    tracing::debug!(record_key = key, "Sending message to process later");
    producer
        .send(record, Timeout::Never)
        .await
        .map_err(|(e, _)| anyhow::anyhow!("Failed to send message: {:?}", e))?;

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
        .set("enable.auto.commit", "true")
        .create()
        .map_err(|e| anyhow::anyhow!("Consumer creation failed: {:?}", e))?;

    Ok(consumer)
}

pub async fn run_consumer() -> Result<()> {
    let consumer = create_consumer()?;

    consumer.subscribe(&["book_ingestion"])?;

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

                // Create a new root span via tracing:
                let span = tracing::info_span!("book_ingestion", "otel.kind" = "Consumer");

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

                // link the extracted span context to our current root span
                // Two options - set the exctracted span as the parent, or just as a reference
                //span.set_parent(parent_cx);

                // If we don't want to set the parent, and keep this as an independent trace
                // instead link it to the parent span:
                // Assign linked trace from external context
                let link_attributes = vec![opentelemetry::KeyValue::new("somekey", "somevalue")];
                span.add_link_with_attributes(linked_span_context, link_attributes);

                span.in_scope(|| {
                    // Deserialize and process the message
                    if let Ok(book_message) = serde_json::from_str::<BookIngestionMessage>(payload)
                    {
                        info!(
                            book_id = book_message.book_id,
                            partition = m.partition(),
                            offset = m.offset(),
                            "Processing book ingestion message"
                        );
                        background_process_new_book(book_message.book_id);
                    } else {
                        error!("Failed to deserialize message payload");
                    }
                });

                // Commit the message offset
                consumer.commit_message(&m, CommitMode::Async)?;
            }
        }
    }
}
