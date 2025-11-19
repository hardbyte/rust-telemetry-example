use serde::{Deserialize, Serialize};
use sqlx::Type;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct BookCreateInput {
    // Normalized: create a work with a primary author (by id or by name)
    pub work_title: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub primary_author_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub primary_author_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub status: Option<BookStatus>,
}

#[derive(Debug, Serialize, Deserialize, Type, Clone, PartialEq, Default, ToSchema)]
#[sqlx(type_name = "book_status", rename_all = "lowercase")]
pub enum BookStatus {
    #[default]
    Available,
    Borrowed,
    Lost,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow, ToSchema)]
pub struct Book {
    // Represents a denormalized view of a Work with its primary author
    pub id: Uuid,                    // work id
    pub work_id: Uuid,               // explicit work id reference (same as id)
    pub work_title: String,          // works.title
    pub primary_author_id: Uuid,     // authors.id (primary)
    pub primary_author_name: String, // authors.name (primary)
    pub status: BookStatus,          // defaulted to Available (no longer from works table)
}

#[derive(Debug, Default)]
pub struct BookFilterParams {
    pub status: Option<BookStatus>,
    pub primary_author_pattern: Option<String>,
    pub work_title_pattern: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug)]
pub enum SortField {
    Title,
    Author,
    Id,
    Status,
}

#[derive(Debug)]
pub enum SortOrder {
    Asc,
    Desc,
}

#[derive(Debug)]
pub struct BookSearchParams {
    pub search_term: Option<String>,
    pub statuses: Vec<BookStatus>,
    pub min_id: Option<Uuid>,
    pub sort_by: SortField,
    pub sort_order: SortOrder,
    pub page: i64,
    pub per_page: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow, ToSchema)]
pub struct Author {
    pub id: Uuid,
    pub name: String,
    pub sort_name: Option<String>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct AuthorCreateInput {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sort_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct Work {
    pub id: Uuid,
    pub title: String,
    pub original_language: Option<String>,
    pub description: Option<String>,
    pub publication_year: Option<i32>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkCreateInput {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub original_language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub publication_year: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct WorkAuthor {
    pub work_id: Uuid,
    pub author_id: Uuid,
    pub role: String,
    pub primary_author: bool,
    pub ord: Option<i16>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorkAuthorCreateInput {
    pub work_id: Uuid,
    pub author_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub primary_author: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ord: Option<i16>,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct Edition {
    pub id: Uuid,
    pub work_id: Uuid,
    pub isbn: String,
    pub title: Option<String>,
    pub publisher: Option<String>,
    pub publication_date: Option<chrono::NaiveDate>,
    pub language: Option<String>,
    pub page_count: Option<i32>,
    pub format: Option<String>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EditionCreateInput {
    pub work_id: Uuid,
    pub isbn: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub publisher: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub publication_date: Option<chrono::NaiveDate>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub page_count: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub format: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct Series {
    pub id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SeriesCreateInput {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct SeriesWorksAssociation {
    pub series_id: Uuid,
    pub work_id: Uuid,
    pub primary_work: bool,
    pub order_id: Option<i32>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SeriesWorksAssociationCreateInput {
    pub series_id: Uuid,
    pub work_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub primary_work: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub order_id: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct Event {
    pub id: i64,
    pub occurred_at: Option<chrono::DateTime<chrono::Utc>>,
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub headers: serde_json::Value,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub source_service: Option<String>,
    pub version: i32,

    // Outbox fields (optional in queries until all SELECTs are updated)
    #[sqlx(default)]
    pub topic: Option<String>,
    #[sqlx(default)]
    pub published_at: Option<chrono::DateTime<chrono::Utc>>,
    #[sqlx(default)]
    pub publish_attempts: i32,
    #[sqlx(default)]
    pub publish_error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow, ToSchema)]
pub struct BookSearchResult {
    pub work_id: Uuid,
    pub work_title: String,
    pub primary_author_name: String,
    pub rank: f32,
    #[sqlx(default)]
    pub headline: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EventCreateInput {
    pub aggregate_type: String,
    pub aggregate_id: String,
    pub event_type: String,
    pub payload: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub headers: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub trace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub span_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source_service: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub version: Option<i32>,

    // Outbox fields for publishing control/override at creation time (optional)
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub topic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub published_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub publish_attempts: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub publish_error: Option<String>,
}
