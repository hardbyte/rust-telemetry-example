mod db;
mod rest;

use tracing::info;
use tracing_subscriber;
use anyhow::{Result, Ok};
use axum::{Extension, Router};

use sqlx::SqlitePool;
use crate::db::{init_db};

fn router(connection_pool: SqlitePool) -> Router {
    Router::new()
        .nest_service("/books", rest::book_service())
        .layer(Extension(connection_pool))
}

#[tokio::main]
async fn main() -> Result<()>{
    // Load env vars
    dotenv::dotenv().ok();

    // install global collector configured based on RUST_LOG env var.
    tracing_subscriber::fmt::init();

    // Init db
    info!("Setting up Database");
    let connection_pool = init_db().await?;
    let app = router(connection_pool);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    info!("Let's rock and roll");
    axum::serve(listener, app).await.unwrap();


    // let books = get_all_books(&connection_pool).await;
    // let a_book = get_book(&connection_pool, 1).await;
    // println!("Got a book {:?}", a_book);


    Ok(())
}
