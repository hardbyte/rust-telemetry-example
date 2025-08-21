use anyhow::Result;
use rdkafka::admin::{AdminClient, AdminOptions, NewTopic, TopicReplication};
use rdkafka::client::DefaultClientContext;
use rdkafka::error::RDKafkaErrorCode::TopicAlreadyExists;
use rdkafka::ClientConfig;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Create a Kafka AdminClient using KAFKA_BROKER_URL (defaults to kafka:9092)
pub fn create_admin_client() -> Result<AdminClient<DefaultClientContext>> {
    let kafka_broker_url =
        std::env::var("KAFKA_BROKER_URL").unwrap_or_else(|_| "kafka:9092".to_string());

    let admin_client: AdminClient<DefaultClientContext> = ClientConfig::new()
        .set("bootstrap.servers", &kafka_broker_url)
        .create()
        .map_err(|e| anyhow::anyhow!("AdminClient creation error: {:?}", e))?;

    Ok(admin_client)
}

/// Ensure a single topic exists, creating it if missing.
pub async fn ensure_topic_exists(
    admin_client: &AdminClient<DefaultClientContext>,
    topic_name: &str,
) -> Result<()> {
    // Fetch existing topics
    let metadata = admin_client
        .inner()
        .fetch_metadata(None, Duration::from_secs(5))
        .map_err(|e| anyhow::anyhow!("Failed to fetch metadata: {:?}", e))?;

    let topic_exists = metadata.topics().iter().any(|t| t.name() == topic_name);

    if topic_exists {
        debug!(topic = %topic_name, "Kafka topic already exists");
        return Ok(());
    }

    // Topic does not exist, create it
    info!(topic = %topic_name, "Creating Kafka topic");
    let new_topic = NewTopic::new(topic_name, 1, TopicReplication::Fixed(1));

    let res = admin_client
        .create_topics(&[new_topic], &AdminOptions::new())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create topic: {:?}", e))?;

    for result in res {
        match result {
            Ok(topic) => info!(topic = %topic, "Created Kafka topic"),
            Err((topic, err)) => {
                if err == TopicAlreadyExists {
                    info!(topic = %topic, "Kafka topic already exists");
                } else {
                    return Err(anyhow::anyhow!(
                        "Failed to create topic {}: {:?}",
                        topic,
                        err
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Ensure a list of topics exist, creating any that are missing.
pub async fn ensure_topics_exist(
    admin_client: &AdminClient<DefaultClientContext>,
    topic_names: &[&str],
) -> Result<()> {
    // Determine which topics are missing via a single metadata fetch
    let metadata = admin_client
        .inner()
        .fetch_metadata(None, Duration::from_secs(5))
        .map_err(|e| anyhow::anyhow!("Failed to fetch metadata: {:?}", e))?;

    let existing: std::collections::HashSet<_> = metadata
        .topics()
        .iter()
        .map(|t| t.name().to_string())
        .collect();

    let missing: Vec<&str> = topic_names
        .iter()
        .copied()
        .filter(|t| !existing.contains(&t.to_string()))
        .collect();

    if missing.is_empty() {
        debug!("All Kafka topics already exist");
        return Ok(());
    }

    info!(missing = ?missing, "Creating missing Kafka topics");

    let new_topics: Vec<NewTopic> = missing
        .iter()
        .map(|t| NewTopic::new(t, 1, TopicReplication::Fixed(1)))
        .collect();

    let results = admin_client
        .create_topics(&new_topics, &AdminOptions::new())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create topics: {:?}", e))?;

    for result in results {
        match result {
            Ok(topic) => info!(topic = %topic, "Created Kafka topic"),
            Err((topic, err)) => {
                if err == TopicAlreadyExists {
                    info!(topic = %topic, "Kafka topic already exists");
                } else {
                    warn!(topic = %topic, error = ?err, "Failed to create topic");
                }
            }
        }
    }

    Ok(())
}

/// Ensure the outbox topic exists, returning the topic name that will be used.
/// Uses OUTBOX_DEFAULT_TOPIC if set, otherwise "domain.events".
pub async fn ensure_outbox_topic(
    admin_client: &AdminClient<DefaultClientContext>,
) -> Result<String> {
    let topic = std::env::var("OUTBOX_DEFAULT_TOPIC").unwrap_or_else(|_| "domain.events".into());
    ensure_topic_exists(admin_client, &topic).await?;
    Ok(topic)
}
