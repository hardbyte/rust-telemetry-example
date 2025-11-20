use crate::error::{DalError, Result};
use crate::models::{
    Book, BookCreateInput, BookFilterParams, BookSearchParams, BookSearchResult, BookStatus,
};
use sqlx::{Executor, PgPool, Postgres};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use tracing::{debug, warn};
use uuid::Uuid;

pub struct BookRepositoryImpl {
    write_pool: Arc<PgPool>,
    read_pool: Arc<PgPool>,
}

impl BookRepositoryImpl {
    /// Create repository with separate read and write pools
    pub fn new(write_pool: Arc<PgPool>, read_pool: Arc<PgPool>) -> Self {
        Self {
            write_pool,
            read_pool,
        }
    }

    /// Create repository with single pool for both read and write operations
    pub fn single_pool(pool: Arc<PgPool>) -> Self {
        Self {
            write_pool: pool.clone(),
            read_pool: pool,
        }
    }

    /// Get access to the write pool for administrative operations
    pub fn write_pool(&self) -> &Arc<PgPool> {
        &self.write_pool
    }

    /// Executor-aware variant: find a book by id using a provided executor (pool or transaction)
    pub async fn find_by_id_with<'e, E>(&self, exec: E, id: Uuid) -> Result<Option<Book>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        sqlx::query_as!(
            Book,
            r#"
            SELECT
                w.id                         as "id!",
                w.id                         as "work_id!",
                w.title                      as "work_title!",
                a.id                         as "primary_author_id!",
                a.name                       as "primary_author_name!",
                'available'::book_status     as "status!: BookStatus"
            FROM works w
            JOIN work_authors wa
              ON wa.work_id = w.id
             AND wa.primary_author = true
            JOIN authors a
              ON a.id = wa.author_id
            WHERE w.id = $1
            "#,
            id
        )
        .fetch_optional(exec)
        .await
        .map_err(Into::into)
    }

    /// Executor-aware variant: create a new book (normalized: works + authors + association)
    pub async fn create_with(
        &self,
        tx: &mut sqlx::Transaction<'_, Postgres>,
        input: BookCreateInput,
    ) -> Result<Uuid> {
        // 1) Insert the work
        let work_id: Uuid = sqlx::query_scalar(
            r#"
            INSERT INTO works (title)
            VALUES ($1)
            RETURNING id
            "#,
        )
        .bind(&input.work_title)
        .fetch_one(tx.as_mut())
        .await?;

        // 2) Resolve/create primary author
        let author_id = if let Some(id) = input.primary_author_id {
            id
        } else if let Some(name) = input.primary_author_name.clone() {
            sqlx::query_scalar(
                r#"
                INSERT INTO authors (name, sort_name)
                VALUES ($1, NULL)
                ON CONFLICT (name)
                DO UPDATE SET sort_name = COALESCE(EXCLUDED.sort_name, authors.sort_name)
                RETURNING id
                "#,
            )
            .bind(&name)
            .fetch_one(tx.as_mut())
            .await?
        } else {
            return Err(crate::error::DalError::InvalidInput {
                message: "primary_author_id or primary_author_name must be provided".into(),
            });
        };

        // 3) Upsert association as primary author
        sqlx::query(
            r#"
            INSERT INTO work_authors (work_id, author_id, role, primary_author, ord)
            VALUES ($1, $2, 'Author', true, 1)
            ON CONFLICT (work_id, author_id)
            DO UPDATE SET primary_author = true, ord = 1
            "#,
        )
        .bind(work_id)
        .bind(author_id)
        .execute(tx.as_mut())
        .await?;

        Ok(work_id)
    }

    /// Executor-aware variant: update an existing book (normalized: works)
    pub async fn update_with<'e, E>(&self, exec: E, book: Book) -> Result<i32>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let result = sqlx::query(
            r#"
            UPDATE works
            SET
                title = $2
            WHERE id = $1
            "#,
        )
        .bind(book.id)
        .bind(&book.work_title)
        .execute(exec)
        .await?;

        let rows_affected: i32 = result.rows_affected().try_into().unwrap_or(0);
        if rows_affected == 0 {
            warn!("Update operation affected 0 rows - book may not exist");
        } else {
            debug!("Successfully updated book");
        }
        Ok(rows_affected)
    }

    /// Executor-aware variant: delete a book (normalized: works)
    pub async fn delete_with<'e, E>(&self, exec: E, id: Uuid) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let result = sqlx::query("DELETE FROM works WHERE id = $1")
            .bind(id)
            .execute(exec)
            .await?;

        if result.rows_affected() == 0 {
            return Err(DalError::BookNotFound { id });
        }

        Ok(())
    }

    /// Executor-aware variant: bulk create books (normalized: per-item create)
    pub async fn bulk_create_with(
        &self,
        tx: &mut sqlx::Transaction<'_, Postgres>,
        books: &[BookCreateInput],
    ) -> Result<Vec<Uuid>> {
        if books.is_empty() {
            return Ok(Vec::new());
        }

        let mut titles = Vec::with_capacity(books.len());
        let mut author_ids: Vec<Option<Uuid>> = Vec::with_capacity(books.len());
        let mut author_names: Vec<Option<String>> = Vec::with_capacity(books.len());
        let mut distinct_names = BTreeSet::new();

        for input in books {
            titles.push(input.work_title.clone());
            match (input.primary_author_id, input.primary_author_name.clone()) {
                (Some(id), _) => {
                    author_ids.push(Some(id));
                    author_names.push(None);
                }
                (None, Some(name)) => {
                    distinct_names.insert(name.clone());
                    author_ids.push(None);
                    author_names.push(Some(name));
                }
                _ => {
                    return Err(crate::error::DalError::InvalidInput {
                        message: "primary_author_id or primary_author_name must be provided".into(),
                    });
                }
            }
        }

        let inserted_work_ids = sqlx::query!(
            r#"
            WITH input(title) AS (
                SELECT unnest($1::text[])
            )
            INSERT INTO works (title)
            SELECT title FROM input
            RETURNING id
            "#,
            &titles
        )
        .fetch_all(tx.as_mut())
        .await?
        .into_iter()
        .map(|row| row.id)
        .collect::<Vec<Uuid>>();

        if inserted_work_ids.len() != books.len() {
            return Err(crate::error::DalError::InvalidInput {
                message: "unexpected mismatch inserting works".into(),
            });
        }

        let mut name_to_id: HashMap<String, Uuid> = HashMap::new();
        if !distinct_names.is_empty() {
            let name_vec: Vec<String> = distinct_names.iter().cloned().collect();
            let rows = sqlx::query!(
                r#"
                WITH input(name) AS (
                    SELECT DISTINCT val
                    FROM unnest($1::text[]) AS data(val)
                )
                INSERT INTO authors (name, sort_name)
                SELECT name, NULL
                FROM input
                ON CONFLICT (name)
                DO UPDATE
                SET sort_name = COALESCE(EXCLUDED.sort_name, authors.sort_name)
                RETURNING name, id
                "#,
                &name_vec
            )
            .fetch_all(tx.as_mut())
            .await?;

            for row in rows {
                name_to_id.insert(row.name, row.id);
            }
        }

        let mut resolved_author_ids = Vec::with_capacity(books.len());
        for (idx, maybe_id) in author_ids.into_iter().enumerate() {
            if let Some(id) = maybe_id {
                resolved_author_ids.push(id);
                continue;
            }
            let key = author_names[idx]
                .as_ref()
                .expect("author name should be present for unresolved ids");
            if let Some(id) = name_to_id.get(key) {
                resolved_author_ids.push(*id);
            } else {
                // Should not happen unless authors table changed concurrently.
                return Err(crate::error::DalError::InvalidInput {
                    message: format!("failed to resolve author id for name {key}"),
                });
            }
        }

        sqlx::query(
            r#"
            INSERT INTO work_authors (work_id, author_id, role, primary_author, ord)
            SELECT work_id, author_id, 'Author', true, 1
            FROM unnest($1::uuid[], $2::uuid[]) AS t(work_id, author_id)
            ON CONFLICT (work_id, author_id)
            DO UPDATE SET primary_author = true, ord = 1
            "#,
        )
        .bind(&inserted_work_ids)
        .bind(&resolved_author_ids)
        .execute(tx.as_mut())
        .await?;

        Ok(inserted_work_ids)
    }
}

impl BookRepositoryImpl {
    #[tracing::instrument(name = "get_all_books_from_db", level = tracing::Level::DEBUG, skip(self))]
    pub async fn find_all(&self, limit: i64, last_seen: Option<Uuid>) -> Result<Vec<Book>> {
        self.find_all_with(&*self.read_pool, limit, last_seen).await
    }

    pub async fn find_all_with<'e, E>(
        &self,
        exec: E,
        limit: i64,
        last_seen: Option<Uuid>,
    ) -> Result<Vec<Book>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        debug!(
            "Getting books from database with pagination: limit={}, last_seen={:?}",
            limit, last_seen
        );

        if let Some(last_id) = last_seen {
            let books = sqlx::query_as!(
                Book,
                r#"
                SELECT
                    w.id                         as "id!",
                    w.id                         as "work_id!",
                    w.title                      as "work_title!",
                    a.id                         as "primary_author_id!",
                    a.name                       as "primary_author_name!",
                    'available'::book_status     as "status!: _"
                FROM works w
                JOIN work_authors wa
                  ON wa.work_id = w.id
                 AND wa.primary_author = true
                JOIN authors a
                  ON a.id = wa.author_id
                WHERE w.id > $2
                ORDER BY w.id
                LIMIT $1
                "#,
                limit,
                last_id
            )
            .fetch_all(exec)
            .await?;
            return Ok(books);
        }

        let books = sqlx::query_as!(
            Book,
            r#"
            SELECT
                w.id                         as "id!",
                w.id                         as "work_id!",
                w.title                      as "work_title!",
                a.id                         as "primary_author_id!",
                a.name                       as "primary_author_name!",
                'available'::book_status     as "status!: _"
            FROM works w
            JOIN work_authors wa
              ON wa.work_id = w.id
             AND wa.primary_author = true
            JOIN authors a
              ON a.id = wa.author_id
            ORDER BY w.id
            LIMIT $1
            "#,
            limit
        )
        .fetch_all(exec)
        .await?;

        Ok(books)
    }

    pub async fn list_ids(&self, limit: i64, last_seen: Option<Uuid>) -> Result<Vec<Uuid>> {
        self.list_ids_with(&*self.read_pool, limit, last_seen).await
    }

    pub async fn list_ids_with<'e, E>(
        &self,
        exec: E,
        limit: i64,
        last_seen: Option<Uuid>,
    ) -> Result<Vec<Uuid>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        if let Some(last_id) = last_seen {
            let ids = sqlx::query_scalar!(
                r#"
                SELECT
                    w.id
                FROM works w
                WHERE w.id > $2
                ORDER BY w.id
                LIMIT $1
                "#,
                limit,
                last_id
            )
            .fetch_all(exec)
            .await?;
            return Ok(ids);
        }

        let ids = sqlx::query_scalar!(
            r#"
            SELECT
                w.id
            FROM works w
            ORDER BY w.id
            LIMIT $1
            "#,
            limit
        )
        .fetch_all(exec)
        .await?;

        Ok(ids)
    }

    #[tracing::instrument(name = "get_book_by_id", skip(self), fields(book.id = %id, db.operation.name = "select", db.collection.name = "works", db.namespace = "bookapp", db.system.name = "postgresql"))]
    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<Book>> {
        self.find_by_id_with(&*self.read_pool, id).await
    }

    #[tracing::instrument(name = "create_work_with_primary_author", skip(self, input), fields(work.title = %input.work_title, db.operation.name = "insert", db.collection.name = "works", db.namespace = "bookapp", db.system.name = "postgresql"))]
    pub async fn create(&self, input: BookCreateInput) -> Result<Uuid> {
        let mut tx = self.write_pool.begin().await?;
        let work_id = self.create_with(&mut tx, input).await?;
        tx.commit().await?;
        Ok(work_id)
    }

    #[tracing::instrument(name = "update_book_in_db", skip(self), fields(book.id = %book.id, book.work_title = %book.work_title, db.operation.name = "update", db.collection.name = "works", db.namespace = "bookapp", db.system.name = "postgresql"))]
    pub async fn update(&self, book: Book) -> Result<i32> {
        self.update_with(&*self.write_pool, book).await
    }

    #[tracing::instrument(name = "delete_book_from_db", skip(self), fields(book.id = %id, db.operation.name = "delete", db.collection.name = "works", db.namespace = "bookapp", db.system.name = "postgresql"))]
    pub async fn delete(&self, id: Uuid) -> Result<()> {
        self.delete_with(&*self.write_pool, id).await
    }

    #[tracing::instrument(name = "bulk_create_books_in_db", skip(self, books), fields(num_books = books.len(), db.operation.name = "batch_insert", db.collection.name = "works", db.namespace = "bookapp", db.system.name = "postgresql", db.operation.batch.size = books.len()))]
    pub async fn bulk_create(&self, books: &[BookCreateInput]) -> Result<Vec<Uuid>> {
        if books.is_empty() {
            return Ok(Vec::new());
        }

        // Use a single transaction to atomically create all works + author associations
        let mut tx = self.write_pool.begin().await?;

        let ids = self.bulk_create_with(&mut tx, books).await?;

        tx.commit().await?;
        Ok(ids)
    }

    pub async fn find_by_filters(&self, params: BookFilterParams) -> Result<Vec<Book>> {
        self.find_by_filters_with(&*self.read_pool, params).await
    }

    pub async fn find_by_filters_with<'e, E>(
        &self,
        exec: E,
        params: BookFilterParams,
    ) -> Result<Vec<Book>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let limit = params.limit.unwrap_or(100);
        let offset = params.offset.unwrap_or(0);
        let author_pattern = params
            .primary_author_pattern
            .as_ref()
            .map(|pattern| format!("%{}%", pattern));
        let title_pattern = params
            .work_title_pattern
            .as_ref()
            .map(|pattern| format!("%{}%", pattern));

        sqlx::query_as!(
            Book,
            r#"
            SELECT
                w.id                         as "id!",
                w.id                         as "work_id!",
                w.title                      as "work_title!",
                a.id                         as "primary_author_id!",
                a.name                       as "primary_author_name!",
                'available'::book_status     as "status!: BookStatus"
            FROM works w
            JOIN work_authors wa
              ON wa.work_id = w.id
             AND wa.primary_author = true
            JOIN authors a
              ON a.id = wa.author_id
            WHERE
                ($1::text IS NULL OR a.name ILIKE $1)
                AND ($2::text IS NULL OR w.title ILIKE $2)
            ORDER BY w.title, a.name
            LIMIT $3
            OFFSET $4
            "#,
            author_pattern.as_deref(),
            title_pattern.as_deref(),
            limit,
            offset
        )
        .fetch_all(exec)
        .await
        .map_err(Into::into)
    }

    pub async fn search_books(&self, params: BookSearchParams) -> Result<Vec<Book>> {
        self.search_books_with(&*self.read_pool, params).await
    }

    pub async fn search_books_with<'e, E>(
        &self,
        exec: E,
        params: BookSearchParams,
    ) -> Result<Vec<Book>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let offset = (params.page - 1) * params.per_page;

        if let Some(search_term) = params.search_term {
            sqlx::query_as!(
                Book,
                r#"
                SELECT
                    w.id                         as "id!",
                    w.id                         as "work_id!",
                    w.title                      as "work_title!",
                    a.id                         as "primary_author_id!",
                    a.name                       as "primary_author_name!",
                    'available'::book_status     as "status!: BookStatus"
                FROM works w
                JOIN work_authors wa
                  ON wa.work_id = w.id
                 AND wa.primary_author = true
                JOIN authors a
                  ON a.id = wa.author_id
                WHERE
                    w.title ILIKE '%' || $1 || '%'
                    OR a.name  ILIKE '%' || $1 || '%'
                ORDER BY w.title, a.name
                LIMIT $2
                OFFSET $3
                "#,
                search_term,
                params.per_page,
                offset
            )
            .fetch_all(exec)
            .await
            .map_err(Into::into)
        } else {
            sqlx::query_as!(
                Book,
                r#"
                SELECT
                    w.id                         as "id!",
                    w.id                         as "work_id!",
                    w.title                      as "work_title!",
                    a.id                         as "primary_author_id!",
                    a.name                       as "primary_author_name!",
                    'available'::book_status     as "status!: BookStatus"
                FROM works w
                JOIN work_authors wa
                  ON wa.work_id = w.id
                 AND wa.primary_author = true
                JOIN authors a
                  ON a.id = wa.author_id
                ORDER BY w.title, a.name
                LIMIT $1
                OFFSET $2
                "#,
                params.per_page,
                offset
            )
            .fetch_all(exec)
            .await
            .map_err(Into::into)
        }
    }

    #[tracing::instrument(
        name = "full_text_search_books_in_db",
        skip(self),
        fields(
            search.query = %query,
            search.limit = %limit,
            search.result_count,
            search.max_rank,
            search.min_rank,
            search.execution_time_ms,
            db.operation = "search",
            db.collection.name = "book_search_index"
        )
    )]
    pub async fn full_text_search(&self, query: &str, limit: i64) -> Result<Vec<BookSearchResult>> {
        self.full_text_search_with(&*self.read_pool, query, limit)
            .await
    }

    pub async fn full_text_search_with<'e, E>(
        &self,
        exec: E,
        query: &str,
        limit: i64,
    ) -> Result<Vec<BookSearchResult>>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let start_time = std::time::Instant::now();

        tracing::info!(
            search.query = %query,
            search.limit = %limit,
            "Starting full-text search operation"
        );

        let results = sqlx::query_as!(
            BookSearchResult,
            r#"
            WITH ts_input AS (
                SELECT websearch_to_tsquery('english', $1) AS query
            )
            SELECT
                idx.work_id as "work_id!",
                idx.work_title as "work_title!",
                idx.primary_author_name as "primary_author_name!",
                ts_rank(idx.document, ts_input.query)::real as "rank!",
                ts_headline(
                    'english',
                    idx.work_title,
                    ts_input.query,
                    'StartSel=<em>, StopSel=</em>, MinWords=5, MaxWords=10'
                ) as "headline"
            FROM ts_input, book_search_index idx
            WHERE idx.document @@ ts_input.query
            ORDER BY ts_rank(idx.document, ts_input.query) DESC
            LIMIT $2
            "#,
            query,
            limit
        )
        .fetch_all(exec)
        .await?;

        let execution_time = start_time.elapsed();

        // Record search metrics and attributes
        let result_count = results.len();
        let max_rank = results.first().map(|r| r.rank).unwrap_or(0.0);
        let min_rank = results.last().map(|r| r.rank).unwrap_or(0.0);

        // Record span attributes for observability
        tracing::Span::current().record("search.result_count", result_count);
        tracing::Span::current().record("search.max_rank", max_rank as f64);
        tracing::Span::current().record("search.min_rank", min_rank as f64);
        tracing::Span::current().record(
            "search.execution_time_ms",
            execution_time.as_millis() as u64,
        );

        tracing::info!(
            search.result_count = result_count,
            search.max_rank = max_rank,
            search.min_rank = min_rank,
            search.execution_time_ms = execution_time.as_millis(),
            search.has_results = !results.is_empty(),
            "Full-text search completed successfully"
        );

        Ok(results)
    }

    #[tracing::instrument(
        name = "search_index.upsert_work_with",
        skip(self, exec),
        fields(work_id = %work_id)
    )]
    pub async fn upsert_search_index_for_work_with<'e, E>(
        &self,
        exec: E,
        work_id: Uuid,
    ) -> Result<()>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let result = sqlx::query!(
            r#"
            WITH authors_agg AS (
                SELECT
                    wa.work_id,
                    string_agg(a.name, ' ' ORDER BY wa.ord) AS author_names
                FROM work_authors wa
                JOIN authors a ON a.id = wa.author_id
                WHERE wa.work_id = $1
                GROUP BY wa.work_id
            ),
            primary_author AS (
                SELECT DISTINCT ON (wa.work_id)
                    wa.work_id,
                    a.name AS primary_author_name
                FROM work_authors wa
                JOIN authors a ON a.id = wa.author_id
                WHERE wa.work_id = $1
                ORDER BY wa.work_id, (NOT wa.primary_author), wa.ord
            ),
            series_agg AS (
                SELECT
                    swa.work_id,
                    string_agg(s.name, ' ' ORDER BY s.name) AS series_names
                FROM series_works_association swa
                JOIN series s ON s.id = swa.series_id
                WHERE swa.work_id = $1
                GROUP BY swa.work_id
            ),
            payload AS (
                SELECT
                    w.id AS work_id,
                    w.title AS work_title,
                    COALESCE(pa.primary_author_name, w.title) AS primary_author_name,
                    COALESCE(aa.author_names, '') AS author_names,
                    COALESCE(sa.series_names, '') AS series_names
                FROM works w
                LEFT JOIN authors_agg aa ON aa.work_id = w.id
                LEFT JOIN primary_author pa ON pa.work_id = w.id
                LEFT JOIN series_agg sa ON sa.work_id = w.id
                WHERE w.id = $1
            )
            INSERT INTO book_search_index (
                work_id,
                work_title,
                primary_author_name,
                author_names,
                series_names,
                document
            )
            SELECT
                payload.work_id,
                payload.work_title,
                payload.primary_author_name,
                payload.author_names,
                payload.series_names,
                setweight(to_tsvector('english', COALESCE(payload.work_title, '')), 'A') ||
                    setweight(to_tsvector('english', COALESCE(payload.author_names, '')), 'B') ||
                    setweight(to_tsvector('english', COALESCE(payload.series_names, '')), 'C')
            FROM payload
            ON CONFLICT (work_id) DO UPDATE
            SET
                work_title = EXCLUDED.work_title,
                primary_author_name = EXCLUDED.primary_author_name,
                author_names = EXCLUDED.author_names,
                series_names = EXCLUDED.series_names,
                document = EXCLUDED.document,
                updated_at = now()
            "#,
            work_id
        )
        .execute(exec)
        .await?;

        if result.rows_affected() == 0 {
            return Err(DalError::BookNotFound { id: work_id });
        }

        Ok(())
    }

    pub async fn upsert_search_index_for_work(&self, work_id: Uuid) -> Result<()> {
        self.upsert_search_index_for_work_with(&*self.write_pool, work_id)
            .await
    }

    #[tracing::instrument(
        name = "search_index.rebuild",
        skip(self),
        fields(db.operation = "rebuild_search_index")
    )]
    pub async fn rebuild_search_index(&self) -> Result<u64> {
        let pool = self.write_pool.as_ref();
        sqlx::query("TRUNCATE book_search_index")
            .execute(pool)
            .await?;

        let result = sqlx::query!(
            r#"
            WITH authors_agg AS (
                SELECT
                    wa.work_id,
                    string_agg(a.name, ' ' ORDER BY wa.ord) AS author_names
                FROM work_authors wa
                JOIN authors a ON a.id = wa.author_id
                GROUP BY wa.work_id
            ),
            primary_author AS (
                SELECT DISTINCT ON (wa.work_id)
                    wa.work_id,
                    a.name AS primary_author_name
                FROM work_authors wa
                JOIN authors a ON a.id = wa.author_id
                ORDER BY wa.work_id, (NOT wa.primary_author), wa.ord
            ),
            series_agg AS (
                SELECT
                    swa.work_id,
                    string_agg(s.name, ' ' ORDER BY s.name) AS series_names
                FROM series_works_association swa
                JOIN series s ON s.id = swa.series_id
                GROUP BY swa.work_id
            )
            INSERT INTO book_search_index (
                work_id,
                work_title,
                primary_author_name,
                author_names,
                series_names,
                document
            )
            SELECT
                w.id,
                w.title,
                COALESCE(pa.primary_author_name, w.title),
                aa.author_names,
                sa.series_names,
                setweight(to_tsvector('english', COALESCE(w.title, '')), 'A') ||
                    setweight(to_tsvector('english', COALESCE(aa.author_names, '')), 'B') ||
                    setweight(to_tsvector('english', COALESCE(sa.series_names, '')), 'C')
            FROM works w
            LEFT JOIN authors_agg aa ON aa.work_id = w.id
            LEFT JOIN primary_author pa ON pa.work_id = w.id
            LEFT JOIN series_agg sa ON sa.work_id = w.id
            "#,
        )
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{BookFilterParams, BookSearchParams, BookStatus, SortField, SortOrder};
    use sqlx::PgPool;

    #[sqlx::test]
    async fn test_find_all_books(pool: PgPool) {
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool));

        // Create a test book first since there's no seed data
        let input = BookCreateInput {
            work_title: "Find All Test Book".to_string(),
            primary_author_id: None,
            primary_author_name: Some("Find All Test Author".to_string()),
            status: Some(BookStatus::Available),
        };
        repo.create(input).await.unwrap();

        let books = repo.find_all(10, None).await.unwrap();

        assert!(!books.is_empty());
    }

    #[sqlx::test]
    async fn test_list_book_ids(pool: PgPool) {
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool.clone()));

        let input = BookCreateInput {
            work_title: "ID List Book".to_string(),
            primary_author_id: None,
            primary_author_name: Some("ID Author".to_string()),
            status: Some(BookStatus::Available),
        };
        let book_id = repo.create(input).await.unwrap();

        let ids = repo.list_ids(5, None).await.unwrap();

        assert!(
            ids.contains(&book_id),
            "Expected to find recently created ID in the list"
        );
    }

    #[sqlx::test]
    async fn test_create_and_find_book(pool: PgPool) {
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool));

        let input = BookCreateInput {
            work_title: "Test Book".to_string(),
            primary_author_id: None,
            primary_author_name: Some("Test Author".to_string()),
            status: Some(BookStatus::Available),
        };

        let book_id = repo.create(input).await.unwrap();
        assert!(!book_id.is_nil());

        let found_book = repo.find_by_id(book_id).await.unwrap();
        assert!(found_book.is_some());

        let book = found_book.unwrap();
        assert_eq!(book.work_title, "Test Book");
        assert_eq!(book.primary_author_name, "Test Author");
        assert_eq!(book.status, BookStatus::Available);
    }

    #[sqlx::test]
    async fn test_update_book(pool: PgPool) {
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool));

        // Create a book first
        let input = BookCreateInput {
            work_title: "Original Title".to_string(),
            primary_author_id: None,
            primary_author_name: Some("Original Author".to_string()),
            status: Some(BookStatus::Available),
        };
        let book_id = repo.create(input).await.unwrap();

        // Update the book (only title can be updated now - status is always Available)
        let mut current = repo.find_by_id(book_id).await.unwrap().unwrap();
        current.work_title = "Updated Title".to_string();

        let rows_affected = repo.update(current).await.unwrap();
        assert_eq!(rows_affected, 1);

        // Verify the update
        let found_book = repo.find_by_id(book_id).await.unwrap().unwrap();
        assert_eq!(found_book.work_title, "Updated Title");
        assert_eq!(found_book.status, BookStatus::Available); // Status is always Available now
    }

    #[sqlx::test]
    async fn test_delete_book(pool: PgPool) {
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool));

        // Create a book first
        let input = BookCreateInput {
            work_title: "To Be Deleted".to_string(),
            primary_author_id: None,
            primary_author_name: Some("Delete Author".to_string()),
            status: Some(BookStatus::Available),
        };
        let book_id = repo.create(input).await.unwrap();

        // Verify it exists
        assert!(repo.find_by_id(book_id).await.unwrap().is_some());

        // Delete it
        repo.delete(book_id).await.unwrap();

        // Verify it's gone
        assert!(repo.find_by_id(book_id).await.unwrap().is_none());
    }

    #[sqlx::test]
    async fn test_bulk_create(pool: PgPool) {
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool));

        let books = vec![
            BookCreateInput {
                work_title: "Bulk Book 1".to_string(),
                primary_author_id: None,
                primary_author_name: Some("Bulk Author 1".to_string()),
                status: Some(BookStatus::Available),
            },
            BookCreateInput {
                work_title: "Bulk Book 2".to_string(),
                primary_author_id: None,
                primary_author_name: Some("Bulk Author 2".to_string()),
                status: Some(BookStatus::Borrowed),
            },
            BookCreateInput {
                work_title: "Bulk Book 3".to_string(),
                primary_author_id: None,
                primary_author_name: Some("Bulk Author 3".to_string()),
                status: None, // Should default to Available
            },
        ];

        let ids = repo.bulk_create(&books).await.unwrap();
        assert_eq!(ids.len(), 3);

        // Verify all books were created
        for id in ids {
            let book = repo.find_by_id(id).await.unwrap();
            assert!(book.is_some());
        }
    }

    #[sqlx::test]
    async fn test_find_by_filters(pool: PgPool) {
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool));

        // Create test books
        let test_books = vec![
            BookCreateInput {
                work_title: "Filter Test Book 1".to_string(),
                primary_author_id: None,
                primary_author_name: Some("Smith".to_string()),
                status: Some(BookStatus::Available),
            },
            BookCreateInput {
                work_title: "Filter Test Book 2".to_string(),
                primary_author_id: None,
                primary_author_name: Some("Johnson".to_string()),
                status: Some(BookStatus::Borrowed),
            },
        ];

        repo.bulk_create(&test_books).await.unwrap();

        // Status filtering no longer supported - all books show as Available
        // Test that we get all books when no filters are provided
        let params = BookFilterParams {
            ..Default::default()
        };

        let all_books = repo.find_by_filters(params).await.unwrap();
        assert!(!all_books.is_empty());

        // Test filter by primary author pattern
        let params = BookFilterParams {
            primary_author_pattern: Some("Smith".to_string()),
            ..Default::default()
        };

        let smith_books = repo.find_by_filters(params).await.unwrap();
        assert!(!smith_books.is_empty());
        assert!(smith_books
            .iter()
            .all(|book| book.primary_author_name.contains("Smith")));
    }

    #[sqlx::test]
    async fn test_search_books(pool: PgPool) {
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool));

        // Create a test book with unique content
        let unique_title = "Unique Search Test Book";
        let test_book = BookCreateInput {
            work_title: unique_title.to_string(),
            primary_author_id: None,
            primary_author_name: Some("Search Author".to_string()),
            status: Some(BookStatus::Available),
        };

        repo.create(test_book).await.unwrap();

        // Search for it
        let search_params = BookSearchParams {
            search_term: Some("Unique Search".to_string()),
            statuses: vec![],
            min_id: None,
            sort_by: SortField::Title,
            sort_order: SortOrder::Asc,
            page: 1,
            per_page: 10,
        };

        let results = repo.search_books(search_params).await.unwrap();
        assert!(!results.is_empty());
        assert!(results
            .iter()
            .any(|book| book.work_title.contains("Unique Search")));
    }

    #[sqlx::test]
    async fn test_full_text_search(pool: PgPool) {
        let repo = BookRepositoryImpl::single_pool(Arc::new(pool));

        // Create test data that will be indexed in the materialized view
        let test_books = vec![
            BookCreateInput {
                work_title: "Harry Potter and the Philosopher's Stone".to_string(),
                primary_author_id: None,
                primary_author_name: Some("J.K. Rowling".to_string()),
                status: Some(BookStatus::Available),
            },
            BookCreateInput {
                work_title: "A Game of Thrones".to_string(),
                primary_author_id: None,
                primary_author_name: Some("George R.R. Martin".to_string()),
                status: Some(BookStatus::Available),
            },
            BookCreateInput {
                work_title: "The Fellowship of the Ring".to_string(),
                primary_author_id: None,
                primary_author_name: Some("J.R.R. Tolkien".to_string()),
                status: Some(BookStatus::Available),
            },
        ];

        repo.bulk_create(&test_books).await.unwrap();
        repo.rebuild_search_index().await.unwrap();

        // Test search by title
        let results = repo.full_text_search("Harry Potter", 10).await.unwrap();
        assert!(!results.is_empty());
        assert!(results
            .iter()
            .any(|r| r.work_title.contains("Harry Potter")));
        assert!(results[0].rank > 0.0);

        // Test search by author
        let results = repo.full_text_search("Rowling", 10).await.unwrap();
        assert!(!results.is_empty());
        assert!(results
            .iter()
            .any(|r| r.primary_author_name.contains("Rowling")));

        // Test search with multiple terms
        let results = repo.full_text_search("Game Thrones", 10).await.unwrap();
        assert!(!results.is_empty());
        assert!(results
            .iter()
            .any(|r| r.work_title.contains("Game of Thrones")));

        // Test search with no results
        let results = repo
            .full_text_search("nonexistent book title", 10)
            .await
            .unwrap();
        assert!(results.is_empty());

        // Test search ordering by relevance (rank)
        let results = repo.full_text_search("Ring", 10).await.unwrap();
        if results.len() > 1 {
            // Results should be ordered by rank descending
            assert!(results[0].rank >= results[1].rank);
        }
    }
}
