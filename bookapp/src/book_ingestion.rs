use anyhow::Result;
use opentelemetry::{global, propagation::Injector, Context as OtelContext};
use rdkafka::message::Header;
use rdkafka::util::Timeout;
use rdkafka::{
    config::ClientConfig,
    message::OwnedHeaders,
    producer::{FutureProducer, FutureRecord},
};
use serde::{Deserialize, Serialize};

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
