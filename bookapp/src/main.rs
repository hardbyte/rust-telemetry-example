mod db;
mod rest;
mod tracing_config;
mod reqwest_traced_client;

use opentelemetry::global;

use tracing_subscriber;

use anyhow::{Ok, Result};
use axum::{Extension, Router};
use axum_tracing_opentelemetry::middleware::{OtelAxumLayer, OtelInResponseLayer};
use tower::{ServiceBuilder};
use tower_http::trace::{DefaultMakeSpan, TraceLayer};

use sqlx::SqlitePool;
use tracing::info;
use crate::db::init_db;


fn router(connection_pool: SqlitePool) -> Router {
    Router::new()
        .nest_service("/books", rest::book_service())
        .layer(Extension(connection_pool))

        // This layer creates a new Tracing span called "request" for each request,
        // it logs headers etc but on its own doesn't do the OTEL trace context propagation.
        // .layer(ServiceBuilder::new().layer(
        //     TraceLayer::new_for_http()
        //         .make_span_with(DefaultMakeSpan::new()
        //             .include_headers(true)
        //             .level(tracing::Level::INFO))
        //
        // ))

        // include trace context as header into the response
        .layer(OtelInResponseLayer::default())
        // start OpenTelemetry trace on incoming request
        // as long as not filtered out!
        .layer(OtelAxumLayer::default())

        // Other non-traced routes can go after this:
        //.route("/health", get(health)) // request processed without span / trace
}


#[tokio::main]
async fn main() -> Result<()>{
    // Load env vars
    dotenv::dotenv().ok();

    tracing_config::init_tracing();

    // Init db
    info!("Setting up Database");
    let connection_pool = init_db().await?;
    let app = router(connection_pool);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await.unwrap();
    info!("Let's rock and roll");
    axum::serve(listener, app).await.unwrap();


    // let books = get_all_books(&connection_pool).await;
    // let a_book = get_book(&connection_pool, 1).await;
    // println!("Got a book {:?}", a_book);

    global::shutdown_tracer_provider();

    Ok(())
}
