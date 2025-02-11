use anyhow::{Context, Ok, Result};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row, Type};
use tracing::{debug, info};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BookCreateIn {
    pub title: String,
    pub author: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<BookStatus>,
}

#[derive(Debug, Serialize, Deserialize, Type, Clone)]
#[sqlx(type_name = "book_status", rename_all = "lowercase")]
pub enum BookStatus {
    Available,
    Borrowed,
    Lost,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Book {
    pub id: i32,

    pub title: String,

    pub author: String,

    pub status: BookStatus,
}

pub async fn init_db() -> Result<PgPool> {
    let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    info!(db_url = db_url, "Connecting to database");

    let con_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
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
pub async fn get_all_books(connection_pool: &PgPool) -> Result<Vec<Book>> {
    debug!("Getting all books at debug inside db module");

    Ok(
        sqlx::query_as!(Book, r#"select id, title, author, status as "status: BookStatus" from books order by title, author"#)
            .fetch_all(connection_pool)
            .await?,
    )
}
pub async fn get_book(connection_pool: &PgPool, id: i32) -> Result<Book> {
    Ok(sqlx::query_as!(
        Book,
        r#"
        select
            id,
            title,
            author,
            status as "status!: BookStatus"
        from books
        where id=$1
        "#,
        id
    )
    .fetch_one(connection_pool)
    .await?)
}

pub async fn create_book(
    connection_pool: &PgPool,
    author: String,
    title: String,
    status: Option<BookStatus>,
) -> Result<i32> {
    Ok(sqlx::query!(
        r#"insert into books (title, author, status) VALUES ($1, $2, $3) returning id"#,
        title,
        author,
        status as Option<BookStatus>,
    )
    .fetch_one(connection_pool)
    .await?
    .id)
}
pub async fn delete_book(connection_pool: &PgPool, id: i32) -> Result<()> {
    sqlx::query!("delete from books where id=$1", id)
        .execute(connection_pool)
        .await?;

    Ok(())
}

pub async fn update_book(connection_pool: &PgPool, book: Book) -> Result<i32> {
    let res = sqlx::query!(
        r#"
        update books
        set
            author=$2,
            title=$3,
            status=$4
        where id=$1
        "#,
        book.id,
        book.author,
        book.title,
        // This cast is necessary for the macro to work
        book.status as BookStatus
    )
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
