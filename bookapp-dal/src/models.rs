use serde::{Deserialize, Serialize};
use sqlx::Type;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BookCreateInput {
    pub title: String,
    pub author: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub status: Option<BookStatus>,
}

#[derive(Debug, Serialize, Deserialize, Type, Clone, PartialEq, Default)]
#[sqlx(type_name = "book_status", rename_all = "lowercase")]
pub enum BookStatus {
    #[default]
    Available,
    Borrowed,
    Lost,
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct Book {
    pub id: i32,
    pub title: String,
    pub author: String,
    pub status: BookStatus,
}

#[derive(Debug, Default)]
pub struct BookFilterParams {
    pub status: Option<BookStatus>,
    pub author_pattern: Option<String>,
    pub title_pattern: Option<String>,
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
    pub min_id: Option<i32>,
    pub sort_by: SortField,
    pub sort_order: SortOrder,
    pub page: i64,
    pub per_page: i64,
}