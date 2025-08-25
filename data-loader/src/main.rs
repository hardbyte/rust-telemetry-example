use clap::Parser;
use client::{types::BookCreateIn, Client as BookappClient, ClientState};
use opentelemetry::{trace::TracerProvider, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::SdkTracerProvider;
use rand::prelude::IndexedRandom;
use std::time::Duration;
use tracing::{error, info, warn};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Registry};

/// Bulk load books via the bookapp API using the Progenitor client
#[derive(Parser, Debug)]
#[command(name = "bulk-loader-api")]
#[command(about = "Load books via the bookapp API with distributed tracing")]
struct Args {
    /// Base URL for the bookapp service
    #[arg(long, default_value = "http://localhost:8000")]
    app_url: String,

    /// Number of books to create
    #[arg(short, long, default_value = "100")]
    count: usize,

    /// Delay between requests in milliseconds
    #[arg(short, long, default_value = "50")]
    delay_ms: u64,

    /// Number of concurrent workers
    #[arg(short, long, default_value = "5")]
    workers: usize,

    /// OTLP endpoint for tracing
    #[arg(long, default_value = "http://localhost:4317")]
    otlp_endpoint: String,
}

static SAMPLE_AUTHORS: &[&str] = &[
    "George Orwell",
    "Jane Austen",
    "Mark Twain",
    "Virginia Woolf",
    "Ernest Hemingway",
    "Toni Morrison",
    "F. Scott Fitzgerald",
    "Maya Angelou",
    "Charles Dickens",
    "Harper Lee",
    "J.K. Rowling",
    "Stephen King",
    "Agatha Christie",
    "Isaac Asimov",
    "Kurt Vonnegut",
    "Margaret Atwood",
    "Ray Bradbury",
    "Octavia Butler",
    "Neil Gaiman",
    "Ursula K. Le Guin",
];

static SAMPLE_TITLES: &[&str] = &[
    "The Great Adventure",
    "Mystery of the Lost City",
    "Journey to Tomorrow",
    "The Silent Forest",
    "Echoes of the Past",
    "Dreams in Color",
    "The Last Frontier",
    "Shadows and Light",
    "The Quantum Leap",
    "Rivers of Time",
    "The Digital Divide",
    "Whispers in the Dark",
    "The Cosmic Dance",
    "Tales from the Edge",
    "The Hidden Truth",
    "Beyond the Horizon",
    "The Perfect Storm",
    "Reflections in Glass",
    "The Endless Night",
    "Sunrise Over Tomorrow",
];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = Args::parse();

    // Initialize tracing with OpenTelemetry
    init_tracing(&args.otlp_endpoint).await?;

    info!(
        "🚀 Starting bulk loader with {} workers to create {} books",
        args.workers, args.count
    );
    info!("📡 Target service: {}", args.app_url);
    info!("⏱️ Delay between requests: {}ms", args.delay_ms);

    // Create Progenitor client with OpenTelemetry context injection built-in
    let client_state = ClientState::default();
    let bookapp_client = BookappClient::new(&args.app_url, client_state);

    info!("✅ Bookapp client initialized with automatic tracing");

    // Test connectivity first
    // test_connectivity(&bookapp_client).await?;
    info!("⏩ Skipping connectivity test for now...");

    // Generate sample books
    let books = generate_sample_books(args.count);
    info!("📚 Generated {} sample books for ingestion", books.len());

    // Create books using concurrent workers
    let chunk_size = (args.count + args.workers - 1) / args.workers; // Round up division
    let mut tasks = Vec::new();

    for (worker_id, book_chunk) in books.chunks(chunk_size).enumerate() {
        let client = BookappClient::new(&args.app_url, ClientState::default());
        let books_to_process = book_chunk.to_vec();
        let delay = Duration::from_millis(args.delay_ms);

        let task = tokio::spawn(async move {
            process_books_worker(worker_id, client, books_to_process, delay).await
        });

        tasks.push(task);
    }

    // Wait for all workers to complete and collect results
    let mut total_created = 0;
    let mut total_failed = 0;

    for task in tasks {
        let (created, failed) = task.await??;
        total_created += created;
        total_failed += failed;
    }

    info!(
        "📊 Bulk loading completed! Created: {}, Failed: {}, Total: {}",
        total_created,
        total_failed,
        total_created + total_failed
    );

    if total_failed > 0 {
        warn!("⚠️ {} books failed to create", total_failed);
    }

    // Shut down tracing
    // Note: In OpenTelemetry 0.30+, we should shutdown the provider explicitly
    // but we need to keep a reference to it for proper shutdown

    info!("✅ Bulk loader completed successfully");

    // Shutdown the tracer provider to ensure all spans are exported
    info!("🔄 Shutting down tracer provider...");
    // Give the batch processor time to export spans
    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    info!("✅ Tracer provider shutdown wait completed");

    Ok(())
}

async fn init_tracing(otlp_endpoint: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!("🔧 Initializing OpenTelemetry tracing: {}", otlp_endpoint);

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(otlp_endpoint)
        .with_timeout(std::time::Duration::from_secs(5))
        .build()?;

    // Configure BatchSpanProcessor with fast export settings
    let batch_config = opentelemetry_sdk::trace::BatchConfigBuilder::default()
        .with_max_queue_size(512)
        .with_scheduled_delay(std::time::Duration::from_millis(500)) // Fast export
        .with_max_export_batch_size(256)
        .build();

    let batch_processor = opentelemetry_sdk::trace::BatchSpanProcessor::builder(exporter)
        .with_batch_config(batch_config)
        .build();

    let trace_provider = SdkTracerProvider::builder()
        .with_span_processor(batch_processor)
        .with_resource(
            opentelemetry_sdk::Resource::builder()
                .with_attributes(vec![KeyValue::new("service.name", "bulk-loader-api")])
                .build(),
        )
        .build();

    opentelemetry::global::set_tracer_provider(trace_provider.clone());

    let tracer = trace_provider.tracer("bulk-loader-api");
    let telemetry_layer = OpenTelemetryLayer::new(tracer);

    Registry::default()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(telemetry_layer)
        .init();

    info!("✅ Tracing initialized");
    Ok(())
}

#[tracing::instrument(skip(client))]
async fn test_connectivity(
    client: &BookappClient,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!("🔍 Testing connectivity to bookapp service");

    match client.get_all_books().send().await {
        Ok(response) => {
            if response.status().is_success() {
                info!("✅ Successfully connected to bookapp service");
                Ok(())
            } else {
                error!("❌ Bookapp service returned error: {}", response.status());
                Err(format!("Service returned status: {}", response.status()).into())
            }
        }
        Err(e) => {
            error!("❌ Failed to connect to bookapp service: {}", e);
            Err(e.into())
        }
    }
}

fn generate_sample_books(count: usize) -> Vec<BookCreateIn> {
    let mut rng = rand::rng();
    let mut books = Vec::with_capacity(count);

    for i in 0..count {
        let author = SAMPLE_AUTHORS.choose(&mut rng).unwrap().to_string();
        let base_title = SAMPLE_TITLES.choose(&mut rng).unwrap().to_string();
        let title = if count > SAMPLE_TITLES.len() {
            format!("{} #{}", base_title, i + 1)
        } else {
            base_title
        };

        books.push(BookCreateIn {
            work_title: title,
            primary_author_name: Some(author),
            primary_author_id: None,
            status: None,
        });
    }

    books
}

#[tracing::instrument(skip(client, books))]
async fn process_books_worker(
    worker_id: usize,
    client: BookappClient,
    books: Vec<BookCreateIn>,
    delay: Duration,
) -> Result<(usize, usize), Box<dyn std::error::Error + Send + Sync>> {
    info!("👷 Worker {} processing {} books", worker_id, books.len());

    let mut created = 0;
    let mut failed = 0;

    for (idx, book) in books.into_iter().enumerate() {
        match create_book_request(&client, book).await {
            Ok(book_id) => {
                created += 1;
                info!(
                    "✅ Worker {} created book #{}: ID {}",
                    worker_id,
                    idx + 1,
                    book_id
                );
            }
            Err(e) => {
                failed += 1;
                error!(
                    "❌ Worker {} failed to create book #{}: {}",
                    worker_id,
                    idx + 1,
                    e
                );
            }
        }

        // Add delay between requests to avoid overwhelming the service
        if delay.as_millis() > 0 {
            tokio::time::sleep(delay).await;
        }
    }

    info!(
        "📊 Worker {} completed: Created {}, Failed {}",
        worker_id, created, failed
    );

    Ok((created, failed))
}

#[tracing::instrument(skip(client), fields(book_author = book.primary_author_name.as_deref().unwrap_or("unknown"), book_title = %book.work_title))]
async fn create_book_request(
    client: &BookappClient,
    book: BookCreateIn,
) -> Result<i32, Box<dyn std::error::Error + Send + Sync>> {
    let response = client.create_book().body(book).send().await?;

    if !response.status().is_success() {
        return Err(format!("API returned status: {}", response.status()).into());
    }

    use futures::StreamExt;
    let mut stream = response.into_inner();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        bytes.extend_from_slice(&chunk?);
    }
    let response_text = String::from_utf8(bytes)?;
    let book_id: i32 = response_text.parse()?;
    Ok(book_id)
}
