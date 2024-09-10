use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Row, SqlitePool};
use anyhow::{Result, Ok, Context};
use tracing::{debug, info, info_span, span};
// #[derive(serde::Deserialize)]
// struct WithID<T> {
//     pub(crate) id: i32,
//
//     #[serde(flatten)]
//     pub(crate) inner: T,
// }

#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct BookCreateIn {
    pub title: String,
    pub author: String,
}

// pub type Book = WithID<BookCreateIn>;


#[derive(Debug, Serialize, Deserialize, FromRow, Clone)]
pub struct Book {
    pub id: i32,

    pub title: String,

    pub author: String,
}

pub async fn init_db() -> Result<SqlitePool> {
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_|"sqlite::memory:".to_string());
    info!("Connecting to database at {}", db_url);
    let con_pool = SqlitePool::connect(&db_url)
        .await
        .context("Failed to connect to the database")?;

    debug!("Running migrations");
    sqlx::migrate!()
        .run(&con_pool)
        .await
        .context("Failed to run migrations")?;

    Ok(con_pool)
}

#[tracing::instrument( name = "get_all_books_from_db", level = tracing::Level::DEBUG )]
pub async fn get_all_books(connection_pool: &SqlitePool) -> Result<Vec<Book>> {
    debug!("Getting all books at debug inside db module");

    Ok(
        sqlx::query_as::<_, Book>("select * from books order by title, author")
            .fetch_all(connection_pool)
            .await?,
    )



}
pub async fn get_book(connection_pool: &SqlitePool, id: i32) -> Result<Book> {
    Ok(
        sqlx::query_as::<_, Book>("select * from books where id=$1")
            .bind(id)
            .fetch_one(connection_pool)
            .await?,
    )
}

pub async fn create_book(connection_pool: &SqlitePool, author: String, title: String) -> Result<i32> {
    Ok(
        sqlx::query("insert into books (title, author) VALUES ($1, $2) returning id")
            .bind(title)
            .bind(author)
            .fetch_one(connection_pool)
            .await?
            .get(0)
    )
}
pub async fn delete_book(connection_pool: &SqlitePool, id: i32) -> Result<()> {
    sqlx::query("delete from books where id=$1")
        .bind(id)
        .execute(connection_pool)
        .await?;

    Ok(())
}

pub async fn update_book(connection_pool: &SqlitePool, book: Book) -> Result<i32> {
    let res = sqlx::query("update books set author=$2, title=$3 where id=$1")
        .bind(book.id)
        .bind(book.author)
        .bind(book.title)
        .execute(connection_pool)
        .await?;

    Ok(res.rows_affected().try_into().unwrap())
}

#[cfg(test)]
mod test {
    use super::*;

    #[sqlx::test]
    async fn get_all() {
        dotenv::dotenv().ok();
        let con = init_db().await.unwrap();
        let all_books = get_all_books(&con).await.unwrap();
        assert!(!all_books.is_empty());
        assert_eq!(all_books.len(), 92);
    }
}