mod db;
mod rest;
mod tracing_config;

use opentelemetry::global;

use tracing_subscriber;

use anyhow::{Ok, Result};
use axum::{Extension, Router};
use tower::{Layer, ServiceBuilder};
use tower_http::trace::{DefaultMakeSpan, TraceLayer};

use sqlx::SqlitePool;
use tracing::info;
use crate::db::init_db;


fn router(connection_pool: SqlitePool) -> Router {
    Router::new()
        .nest_service("/books", rest::book_service())
        .layer(Extension(connection_pool))
        .layer(ServiceBuilder::new().layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new()
                    .include_headers(true)
                    .level(tracing::Level::INFO))
        ))
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
