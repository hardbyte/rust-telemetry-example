use anyhow::Result;
use rdkafka::admin::{AdminClient, AdminOptions, NewTopic, TopicReplication};
use rdkafka::client::DefaultClientContext;
use rdkafka::error::RDKafkaErrorCode::TopicAlreadyExists;
use rdkafka::ClientConfig;
use std::time::Duration;

pub fn create_admin_client() -> Result<AdminClient<DefaultClientContext>> {
    let kafka_broker_url =
        std::env::var("KAFKA_BROKER_URL").unwrap_or_else(|_| "kafka:9092".to_string());

    let admin_client: AdminClient<DefaultClientContext> = ClientConfig::new()
        .set("bootstrap.servers", &kafka_broker_url)
        .create()
        .map_err(|e| anyhow::anyhow!("AdminClient creation error: {:?}", e))?;

    Ok(admin_client)
}

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
        // Topic already exists
        return Ok(());
    }

    // Topic does not exist, create it
    let new_topic = NewTopic::new(topic_name, 1, TopicReplication::Fixed(1));

    let res = admin_client
        .create_topics(&[new_topic], &AdminOptions::new())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create topic: {:?}", e))?;

    for result in res {
        match result {
            Ok(topic) => tracing::info!("Created topic: {}", topic),
            Err((topic, err)) => {
                if err == TopicAlreadyExists {
                    tracing::info!("Topic {} already exists", topic);
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
