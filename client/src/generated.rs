#![allow(clippy::all)]
#[allow(unused_imports)]
use progenitor_client::{encode_path, ClientHooks, OperationInfo, RequestBuilderExt};
#[allow(unused_imports)]
pub use progenitor_client::{ByteStream, ClientInfo, Error, ResponseValue};
/// Types used as operation parameters and responses.
#[allow(clippy::all)]
pub mod types {
    /// Error types.
    pub mod error {
        /// Error from a `TryFrom` or `FromStr` implementation.
        pub struct ConversionError(::std::borrow::Cow<'static, str>);
        impl ::std::error::Error for ConversionError {}
        impl ::std::fmt::Display for ConversionError {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> Result<(), ::std::fmt::Error> {
                ::std::fmt::Display::fmt(&self.0, f)
            }
        }
        impl ::std::fmt::Debug for ConversionError {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> Result<(), ::std::fmt::Error> {
                ::std::fmt::Debug::fmt(&self.0, f)
            }
        }
        impl From<&'static str> for ConversionError {
            fn from(value: &'static str) -> Self {
                Self(value.into())
            }
        }
        impl From<String> for ConversionError {
            fn from(value: String) -> Self {
                Self(value.into())
            }
        }
    }
    ///`Book`
    ///
    /// <details><summary>JSON schema</summary>
    ///
    /// ```json
    ///{
    ///  "type": "object",
    ///  "required": [
    ///    "id",
    ///    "primary_author_id",
    ///    "primary_author_name",
    ///    "status",
    ///    "work_id",
    ///    "work_title"
    ///  ],
    ///  "properties": {
    ///    "id": {
    ///      "type": "integer",
    ///      "format": "int32"
    ///    },
    ///    "primary_author_id": {
    ///      "type": "integer",
    ///      "format": "int32"
    ///    },
    ///    "primary_author_name": {
    ///      "type": "string"
    ///    },
    ///    "status": {
    ///      "type": "string",
    ///      "enum": [
    ///        "Available",
    ///        "Borrowed",
    ///        "Lost"
    ///      ]
    ///    },
    ///    "work_id": {
    ///      "type": "integer",
    ///      "format": "int32"
    ///    },
    ///    "work_title": {
    ///      "type": "string"
    ///    }
    ///  }
    ///}
    /// ```
    /// </details>
    #[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
    pub struct Book {
        pub id: i32,
        pub primary_author_id: i32,
        pub primary_author_name: ::std::string::String,
        pub status: BookStatus,
        pub work_id: i32,
        pub work_title: ::std::string::String,
    }
    impl ::std::convert::From<&Book> for Book {
        fn from(value: &Book) -> Self {
            value.clone()
        }
    }
    impl Book {
        pub fn builder() -> builder::Book {
            Default::default()
        }
    }
    ///`BookCreateIn`
    ///
    /// <details><summary>JSON schema</summary>
    ///
    /// ```json
    ///{
    ///  "type": "object",
    ///  "required": [
    ///    "work_title"
    ///  ],
    ///  "properties": {
    ///    "primary_author_id": {
    ///      "type": "integer",
    ///      "format": "int32"
    ///    },
    ///    "primary_author_name": {
    ///      "type": "string"
    ///    },
    ///    "status": {
    ///      "type": "string",
    ///      "enum": [
    ///        "Available",
    ///        "Borrowed",
    ///        "Lost"
    ///      ]
    ///    },
    ///    "work_title": {
    ///      "type": "string"
    ///    }
    ///  }
    ///}
    /// ```
    /// </details>
    #[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
    pub struct BookCreateIn {
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        pub primary_author_id: ::std::option::Option<i32>,
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        pub primary_author_name: ::std::option::Option<::std::string::String>,
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        pub status: ::std::option::Option<BookCreateInStatus>,
        pub work_title: ::std::string::String,
    }
    impl ::std::convert::From<&BookCreateIn> for BookCreateIn {
        fn from(value: &BookCreateIn) -> Self {
            value.clone()
        }
    }
    impl BookCreateIn {
        pub fn builder() -> builder::BookCreateIn {
            Default::default()
        }
    }
    ///`BookCreateInStatus`
    ///
    /// <details><summary>JSON schema</summary>
    ///
    /// ```json
    ///{
    ///  "type": "string",
    ///  "enum": [
    ///    "Available",
    ///    "Borrowed",
    ///    "Lost"
    ///  ]
    ///}
    /// ```
    /// </details>
    #[derive(
        ::serde::Deserialize,
        ::serde::Serialize,
        Clone,
        Copy,
        Debug,
        Eq,
        Hash,
        Ord,
        PartialEq,
        PartialOrd,
    )]
    pub enum BookCreateInStatus {
        Available,
        Borrowed,
        Lost,
    }
    impl ::std::convert::From<&Self> for BookCreateInStatus {
        fn from(value: &BookCreateInStatus) -> Self {
            value.clone()
        }
    }
    impl ::std::fmt::Display for BookCreateInStatus {
        fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
            match *self {
                Self::Available => write!(f, "Available"),
                Self::Borrowed => write!(f, "Borrowed"),
                Self::Lost => write!(f, "Lost"),
            }
        }
    }
    impl ::std::str::FromStr for BookCreateInStatus {
        type Err = self::error::ConversionError;
        fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
            match value {
                "Available" => Ok(Self::Available),
                "Borrowed" => Ok(Self::Borrowed),
                "Lost" => Ok(Self::Lost),
                _ => Err("invalid value".into()),
            }
        }
    }
    impl ::std::convert::TryFrom<&str> for BookCreateInStatus {
        type Error = self::error::ConversionError;
        fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
            value.parse()
        }
    }
    impl ::std::convert::TryFrom<&::std::string::String> for BookCreateInStatus {
        type Error = self::error::ConversionError;
        fn try_from(
            value: &::std::string::String,
        ) -> ::std::result::Result<Self, self::error::ConversionError> {
            value.parse()
        }
    }
    impl ::std::convert::TryFrom<::std::string::String> for BookCreateInStatus {
        type Error = self::error::ConversionError;
        fn try_from(
            value: ::std::string::String,
        ) -> ::std::result::Result<Self, self::error::ConversionError> {
            value.parse()
        }
    }
    ///`BookSearchResult`
    ///
    /// <details><summary>JSON schema</summary>
    ///
    /// ```json
    ///{
    ///  "type": "object",
    ///  "required": [
    ///    "primary_author_name",
    ///    "rank",
    ///    "work_id",
    ///    "work_title"
    ///  ],
    ///  "properties": {
    ///    "headline": {
    ///      "type": "string"
    ///    },
    ///    "primary_author_name": {
    ///      "type": "string"
    ///    },
    ///    "rank": {
    ///      "type": "number",
    ///      "format": "float"
    ///    },
    ///    "work_id": {
    ///      "type": "integer",
    ///      "format": "int32"
    ///    },
    ///    "work_title": {
    ///      "type": "string"
    ///    }
    ///  }
    ///}
    /// ```
    /// </details>
    #[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
    pub struct BookSearchResult {
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        pub headline: ::std::option::Option<::std::string::String>,
        pub primary_author_name: ::std::string::String,
        pub rank: f32,
        pub work_id: i32,
        pub work_title: ::std::string::String,
    }
    impl ::std::convert::From<&BookSearchResult> for BookSearchResult {
        fn from(value: &BookSearchResult) -> Self {
            value.clone()
        }
    }
    impl BookSearchResult {
        pub fn builder() -> builder::BookSearchResult {
            Default::default()
        }
    }
    ///`BookStatus`
    ///
    /// <details><summary>JSON schema</summary>
    ///
    /// ```json
    ///{
    ///  "type": "string",
    ///  "enum": [
    ///    "Available",
    ///    "Borrowed",
    ///    "Lost"
    ///  ]
    ///}
    /// ```
    /// </details>
    #[derive(
        ::serde::Deserialize,
        ::serde::Serialize,
        Clone,
        Copy,
        Debug,
        Eq,
        Hash,
        Ord,
        PartialEq,
        PartialOrd,
    )]
    pub enum BookStatus {
        Available,
        Borrowed,
        Lost,
    }
    impl ::std::convert::From<&Self> for BookStatus {
        fn from(value: &BookStatus) -> Self {
            value.clone()
        }
    }
    impl ::std::fmt::Display for BookStatus {
        fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
            match *self {
                Self::Available => write!(f, "Available"),
                Self::Borrowed => write!(f, "Borrowed"),
                Self::Lost => write!(f, "Lost"),
            }
        }
    }
    impl ::std::str::FromStr for BookStatus {
        type Err = self::error::ConversionError;
        fn from_str(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
            match value {
                "Available" => Ok(Self::Available),
                "Borrowed" => Ok(Self::Borrowed),
                "Lost" => Ok(Self::Lost),
                _ => Err("invalid value".into()),
            }
        }
    }
    impl ::std::convert::TryFrom<&str> for BookStatus {
        type Error = self::error::ConversionError;
        fn try_from(value: &str) -> ::std::result::Result<Self, self::error::ConversionError> {
            value.parse()
        }
    }
    impl ::std::convert::TryFrom<&::std::string::String> for BookStatus {
        type Error = self::error::ConversionError;
        fn try_from(
            value: &::std::string::String,
        ) -> ::std::result::Result<Self, self::error::ConversionError> {
            value.parse()
        }
    }
    impl ::std::convert::TryFrom<::std::string::String> for BookStatus {
        type Error = self::error::ConversionError;
        fn try_from(
            value: ::std::string::String,
        ) -> ::std::result::Result<Self, self::error::ConversionError> {
            value.parse()
        }
    }
    /// Types for composing complex structures.
    pub mod builder {
        #[derive(Clone, Debug)]
        pub struct Book {
            id: ::std::result::Result<i32, ::std::string::String>,
            primary_author_id: ::std::result::Result<i32, ::std::string::String>,
            primary_author_name:
                ::std::result::Result<::std::string::String, ::std::string::String>,
            status: ::std::result::Result<super::BookStatus, ::std::string::String>,
            work_id: ::std::result::Result<i32, ::std::string::String>,
            work_title: ::std::result::Result<::std::string::String, ::std::string::String>,
        }
        impl ::std::default::Default for Book {
            fn default() -> Self {
                Self {
                    id: Err("no value supplied for id".to_string()),
                    primary_author_id: Err("no value supplied for primary_author_id".to_string()),
                    primary_author_name: Err(
                        "no value supplied for primary_author_name".to_string()
                    ),
                    status: Err("no value supplied for status".to_string()),
                    work_id: Err("no value supplied for work_id".to_string()),
                    work_title: Err("no value supplied for work_title".to_string()),
                }
            }
        }
        impl Book {
            pub fn id<T>(mut self, value: T) -> Self
            where
                T: ::std::convert::TryInto<i32>,
                T::Error: ::std::fmt::Display,
            {
                self.id = value
                    .try_into()
                    .map_err(|e| format!("error converting supplied value for id: {e}"));
                self
            }
            pub fn primary_author_id<T>(mut self, value: T) -> Self
            where
                T: ::std::convert::TryInto<i32>,
                T::Error: ::std::fmt::Display,
            {
                self.primary_author_id = value.try_into().map_err(|e| {
                    format!(
                        "error converting supplied value for primary_author_id: {}",
                        e
                    )
                });
                self
            }
            pub fn primary_author_name<T>(mut self, value: T) -> Self
            where
                T: ::std::convert::TryInto<::std::string::String>,
                T::Error: ::std::fmt::Display,
            {
                self.primary_author_name = value.try_into().map_err(|e| {
                    format!(
                        "error converting supplied value for primary_author_name: {}",
                        e
                    )
                });
                self
            }
            pub fn status<T>(mut self, value: T) -> Self
            where
                T: ::std::convert::TryInto<super::BookStatus>,
                T::Error: ::std::fmt::Display,
            {
                self.status = value
                    .try_into()
                    .map_err(|e| format!("error converting supplied value for status: {}", e));
                self
            }
            pub fn work_id<T>(mut self, value: T) -> Self
            where
                T: ::std::convert::TryInto<i32>,
                T::Error: ::std::fmt::Display,
            {
                self.work_id = value
                    .try_into()
                    .map_err(|e| format!("error converting supplied value for work_id: {}", e));
                self
            }
            pub fn work_title<T>(mut self, value: T) -> Self
            where
                T: ::std::convert::TryInto<::std::string::String>,
                T::Error: ::std::fmt::Display,
            {
                self.work_title = value
                    .try_into()
                    .map_err(|e| format!("error converting supplied value for work_title: {}", e));
                self
            }
        }
        impl ::std::convert::TryFrom<Book> for super::Book {
            type Error = super::error::ConversionError;
            fn try_from(value: Book) -> ::std::result::Result<Self, super::error::ConversionError> {
                Ok(Self {
                    id: value.id?,
                    primary_author_id: value.primary_author_id?,
                    primary_author_name: value.primary_author_name?,
                    status: value.status?,
                    work_id: value.work_id?,
                    work_title: value.work_title?,
                })
            }
        }
        impl ::std::convert::From<super::Book> for Book {
            fn from(value: super::Book) -> Self {
                Self {
                    id: Ok(value.id),
                    primary_author_id: Ok(value.primary_author_id),
                    primary_author_name: Ok(value.primary_author_name),
                    status: Ok(value.status),
                    work_id: Ok(value.work_id),
                    work_title: Ok(value.work_title),
                }
            }
        }
        #[derive(Clone, Debug)]
        pub struct BookCreateIn {
            primary_author_id:
                ::std::result::Result<::std::option::Option<i32>, ::std::string::String>,
            primary_author_name: ::std::result::Result<
                ::std::option::Option<::std::string::String>,
                ::std::string::String,
            >,
            status: ::std::result::Result<
                ::std::option::Option<super::BookCreateInStatus>,
                ::std::string::String,
            >,
            work_title: ::std::result::Result<::std::string::String, ::std::string::String>,
        }
        impl ::std::default::Default for BookCreateIn {
            fn default() -> Self {
                Self {
                    primary_author_id: Ok(Default::default()),
                    primary_author_name: Ok(Default::default()),
                    status: Ok(Default::default()),
                    work_title: Err("no value supplied for work_title".to_string()),
                }
            }
        }
        impl BookCreateIn {
            pub fn primary_author_id<T>(mut self, value: T) -> Self
            where
                T: ::std::convert::TryInto<::std::option::Option<i32>>,
                T::Error: ::std::fmt::Display,
            {
                self.primary_author_id = value.try_into().map_err(|e| {
                    format!(
                        "error converting supplied value for primary_author_id: {}",
                        e
                    )
                });
                self
            }
            pub fn primary_author_name<T>(mut self, value: T) -> Self
            where
                T: ::std::convert::TryInto<::std::option::Option<::std::string::String>>,
                T::Error: ::std::fmt::Display,
            {
                self.primary_author_name = value.try_into().map_err(|e| {
                    format!(
                        "error converting supplied value for primary_author_name: {}",
                        e
                    )
                });
                self
            }
            pub fn status<T>(mut self, value: T) -> Self
            where
                T: ::std::convert::TryInto<::std::option::Option<super::BookCreateInStatus>>,
                T::Error: ::std::fmt::Display,
            {
                self.status = value
                    .try_into()
                    .map_err(|e| format!("error converting supplied value for status: {}", e));
                self
            }
            pub fn work_title<T>(mut self, value: T) -> Self
            where
                T: ::std::convert::TryInto<::std::string::String>,
                T::Error: ::std::fmt::Display,
            {
                self.work_title = value
                    .try_into()
                    .map_err(|e| format!("error converting supplied value for work_title: {}", e));
                self
            }
        }
        impl ::std::convert::TryFrom<BookCreateIn> for super::BookCreateIn {
            type Error = super::error::ConversionError;
            fn try_from(
                value: BookCreateIn,
            ) -> ::std::result::Result<Self, super::error::ConversionError> {
                Ok(Self {
                    primary_author_id: value.primary_author_id?,
                    primary_author_name: value.primary_author_name?,
                    status: value.status?,
                    work_title: value.work_title?,
                })
            }
        }
        impl ::std::convert::From<super::BookCreateIn> for BookCreateIn {
            fn from(value: super::BookCreateIn) -> Self {
                Self {
                    primary_author_id: Ok(value.primary_author_id),
                    primary_author_name: Ok(value.primary_author_name),
                    status: Ok(value.status),
                    work_title: Ok(value.work_title),
                }
            }
        }
        #[derive(Clone, Debug)]
        pub struct BookSearchResult {
            headline: ::std::result::Result<
                ::std::option::Option<::std::string::String>,
                ::std::string::String,
            >,
            primary_author_name:
                ::std::result::Result<::std::string::String, ::std::string::String>,
            rank: ::std::result::Result<f32, ::std::string::String>,
            work_id: ::std::result::Result<i32, ::std::string::String>,
            work_title: ::std::result::Result<::std::string::String, ::std::string::String>,
        }
        impl ::std::default::Default for BookSearchResult {
            fn default() -> Self {
                Self {
                    headline: Ok(Default::default()),
                    primary_author_name: Err(
                        "no value supplied for primary_author_name".to_string()
                    ),
                    rank: Err("no value supplied for rank".to_string()),
                    work_id: Err("no value supplied for work_id".to_string()),
                    work_title: Err("no value supplied for work_title".to_string()),
                }
            }
        }
        impl BookSearchResult {
            pub fn headline<T>(mut self, value: T) -> Self
            where
                T: ::std::convert::TryInto<::std::option::Option<::std::string::String>>,
                T::Error: ::std::fmt::Display,
            {
                self.headline = value
                    .try_into()
                    .map_err(|e| format!("error converting supplied value for headline: {}", e));
                self
            }
            pub fn primary_author_name<T>(mut self, value: T) -> Self
            where
                T: ::std::convert::TryInto<::std::string::String>,
                T::Error: ::std::fmt::Display,
            {
                self.primary_author_name = value.try_into().map_err(|e| {
                    format!(
                        "error converting supplied value for primary_author_name: {}",
                        e
                    )
                });
                self
            }
            pub fn rank<T>(mut self, value: T) -> Self
            where
                T: ::std::convert::TryInto<f32>,
                T::Error: ::std::fmt::Display,
            {
                self.rank = value
                    .try_into()
                    .map_err(|e| format!("error converting supplied value for rank: {}", e));
                self
            }
            pub fn work_id<T>(mut self, value: T) -> Self
            where
                T: ::std::convert::TryInto<i32>,
                T::Error: ::std::fmt::Display,
            {
                self.work_id = value
                    .try_into()
                    .map_err(|e| format!("error converting supplied value for work_id: {}", e));
                self
            }
            pub fn work_title<T>(mut self, value: T) -> Self
            where
                T: ::std::convert::TryInto<::std::string::String>,
                T::Error: ::std::fmt::Display,
            {
                self.work_title = value
                    .try_into()
                    .map_err(|e| format!("error converting supplied value for work_title: {}", e));
                self
            }
        }
        impl ::std::convert::TryFrom<BookSearchResult> for super::BookSearchResult {
            type Error = super::error::ConversionError;
            fn try_from(
                value: BookSearchResult,
            ) -> ::std::result::Result<Self, super::error::ConversionError> {
                Ok(Self {
                    headline: value.headline?,
                    primary_author_name: value.primary_author_name?,
                    rank: value.rank?,
                    work_id: value.work_id?,
                    work_title: value.work_title?,
                })
            }
        }
        impl ::std::convert::From<super::BookSearchResult> for BookSearchResult {
            fn from(value: super::BookSearchResult) -> Self {
                Self {
                    headline: Ok(value.headline),
                    primary_author_name: Ok(value.primary_author_name),
                    rank: Ok(value.rank),
                    work_id: Ok(value.work_id),
                    work_title: Ok(value.work_title),
                }
            }
        }
    }
}
#[derive(Clone, Debug)]
/**Client for Book Service API

API for managing books in the library with distributed tracing support

Version: 1.0.0*/
pub struct Client {
    pub(crate) baseurl: String,
    pub(crate) client: reqwest::Client,
    pub(crate) inner: crate::ClientState,
}
impl Client {
    /// Create a new client.
    ///
    /// `baseurl` is the base URL provided to the internal
    /// `reqwest::Client`, and should include a scheme and hostname,
    /// as well as port and a path stem if applicable.
    pub fn new(baseurl: &str, inner: crate::ClientState) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let client = {
            let dur = std::time::Duration::from_secs(15);
            reqwest::ClientBuilder::new()
                .connect_timeout(dur)
                .timeout(dur)
        };
        #[cfg(target_arch = "wasm32")]
        let client = reqwest::ClientBuilder::new();
        Self::new_with_client(baseurl, client.build().unwrap(), inner)
    }
    /// Construct a new client with an existing `reqwest::Client`,
    /// allowing more control over its configuration.
    ///
    /// `baseurl` is the base URL provided to the internal
    /// `reqwest::Client`, and should include a scheme and hostname,
    /// as well as port and a path stem if applicable.
    pub fn new_with_client(
        baseurl: &str,
        client: reqwest::Client,
        inner: crate::ClientState,
    ) -> Self {
        Self {
            baseurl: baseurl.to_string(),
            client,
            inner,
        }
    }
}
impl ClientInfo<crate::ClientState> for Client {
    fn api_version() -> &'static str {
        "1.0.0"
    }
    fn baseurl(&self) -> &str {
        self.baseurl.as_str()
    }
    fn client(&self) -> &reqwest::Client {
        &self.client
    }
    fn inner(&self) -> &crate::ClientState {
        &self.inner
    }
}
impl ClientHooks<crate::ClientState> for &Client {}
impl Client {
    /**Get all books

    Sends a `GET` request to `/books`

    ```ignore
    let response = client.get_all_books()
        .send()
        .await;
    ```*/
    pub fn get_all_books(&self) -> builder::GetAllBooks {
        builder::GetAllBooks::new(self)
    }
    /**Create a new book

    Sends a `POST` request to `/books/add`

    Arguments:
    - `body`: Data for the new book
    ```ignore
    let response = client.create_book()
        .body(body)
        .send()
        .await;
    ```*/
    pub fn create_book(&self) -> builder::CreateBook {
        builder::CreateBook::new(self)
    }
    /**Get a book by ID

    Sends a `GET` request to `/books/{id}`

    Arguments:
    - `id`: ID of the book
    ```ignore
    let response = client.get_book()
        .id(id)
        .send()
        .await;
    ```*/
    pub fn get_book(&self) -> builder::GetBook {
        builder::GetBook::new(self)
    }
    /**Delete a book by ID

    Sends a `DELETE` request to `/books/{id}`

    Arguments:
    - `id`: ID of the book
    ```ignore
    let response = client.delete_book()
        .id(id)
        .send()
        .await;
    ```*/
    pub fn delete_book(&self) -> builder::DeleteBook {
        builder::DeleteBook::new(self)
    }
    /**Update a book by ID

    Sends a `PATCH` request to `/books/{id}`

    Arguments:
    - `id`: ID of the book
    - `body`: Data to update the book
    ```ignore
    let response = client.update_book()
        .id(id)
        .body(body)
        .send()
        .await;
    ```*/
    pub fn update_book(&self) -> builder::UpdateBook {
        builder::UpdateBook::new(self)
    }
    /**Search books

    Sends a `GET` request to `/books/search`

    Arguments:
    - `limit`: Maximum results
    - `q`: Search query
    ```ignore
    let response = client.search_books()
        .limit(limit)
        .q(q)
        .send()
        .await;
    ```*/
    pub fn search_books(&self) -> builder::SearchBooks {
        builder::SearchBooks::new(self)
    }
}
/// Types for composing operation parameters.
#[allow(clippy::all)]
pub mod builder {
    use super::types;
    #[allow(unused_imports)]
    use super::{
        encode_path, ByteStream, ClientHooks, ClientInfo, Error, OperationInfo, RequestBuilderExt,
        ResponseValue,
    };
    /**Builder for [`Client::get_all_books`]

    [`Client::get_all_books`]: super::Client::get_all_books*/
    #[derive(Debug, Clone)]
    pub struct GetAllBooks<'a> {
        client: &'a super::Client,
    }
    impl<'a> GetAllBooks<'a> {
        pub fn new(client: &'a super::Client) -> Self {
            Self { client: client }
        }
        ///Sends a `GET` request to `/books`
        pub async fn send(self) -> Result<ResponseValue<::std::vec::Vec<types::Book>>, Error<()>> {
            let Self { client } = self;
            let url = format!("{}/books", client.baseurl,);
            let mut header_map = ::reqwest::header::HeaderMap::with_capacity(1usize);
            header_map.append(
                ::reqwest::header::HeaderName::from_static("api-version"),
                ::reqwest::header::HeaderValue::from_static(super::Client::api_version()),
            );
            #[allow(unused_mut)]
            let mut request = client
                .client
                .get(url)
                .header(
                    ::reqwest::header::ACCEPT,
                    ::reqwest::header::HeaderValue::from_static("application/json"),
                )
                .headers(header_map)
                .build()?;
            let info = OperationInfo {
                operation_id: "get_all_books",
            };
            match (|_, request: &mut reqwest::Request| {
                crate::inject_opentelemetry_context_into_request(request);
                Box::pin(async { Ok::<_, Box<dyn std::error::Error>>(()) })
            })(&client.inner, &mut request)
            .await
            {
                Ok(_) => {}
                Err(e) => return Err(Error::Custom(e.to_string())),
            }
            client.pre(&mut request, &info).await?;
            let result = client.exec(request, &info).await;
            client.post(&result, &info).await?;
            let response = result?;
            match response.status().as_u16() {
                200u16 => ResponseValue::from_response(response).await,
                503u16 => Err(Error::ErrorResponse(ResponseValue::empty(response))),
                _ => Err(Error::UnexpectedResponse(response)),
            }
        }
    }
    /**Builder for [`Client::create_book`]

    [`Client::create_book`]: super::Client::create_book*/
    #[derive(Debug, Clone)]
    pub struct CreateBook<'a> {
        client: &'a super::Client,
        body: Result<types::builder::BookCreateIn, String>,
    }
    impl<'a> CreateBook<'a> {
        pub fn new(client: &'a super::Client) -> Self {
            Self {
                client: client,
                body: Ok(::std::default::Default::default()),
            }
        }
        pub fn body<V>(mut self, value: V) -> Self
        where
            V: std::convert::TryInto<types::BookCreateIn>,
            <V as std::convert::TryInto<types::BookCreateIn>>::Error: std::fmt::Display,
        {
            self.body = value
                .try_into()
                .map(From::from)
                .map_err(|s| format!("conversion to `BookCreateIn` for body failed: {s}"));
            self
        }
        pub fn body_map<F>(mut self, f: F) -> Self
        where
            F: std::ops::FnOnce(types::builder::BookCreateIn) -> types::builder::BookCreateIn,
        {
            self.body = self.body.map(f);
            self
        }
        ///Sends a `POST` request to `/books/add`
        pub async fn send(self) -> Result<ResponseValue<ByteStream>, Error<()>> {
            let Self { client, body } = self;
            let body = body
                .and_then(|v| types::BookCreateIn::try_from(v).map_err(|e| e.to_string()))
                .map_err(Error::InvalidRequest)?;
            let url = format!("{}/books/add", client.baseurl,);
            let mut header_map = ::reqwest::header::HeaderMap::with_capacity(1usize);
            header_map.append(
                ::reqwest::header::HeaderName::from_static("api-version"),
                ::reqwest::header::HeaderValue::from_static(super::Client::api_version()),
            );
            #[allow(unused_mut)]
            let mut request = client
                .client
                .post(url)
                .json(&body)
                .headers(header_map)
                .build()?;
            let info = OperationInfo {
                operation_id: "create_book",
            };
            match (|_, request: &mut reqwest::Request| {
                crate::inject_opentelemetry_context_into_request(request);
                Box::pin(async { Ok::<_, Box<dyn std::error::Error>>(()) })
            })(&client.inner, &mut request)
            .await
            {
                Ok(_) => {}
                Err(e) => return Err(Error::Custom(e.to_string())),
            }
            client.pre(&mut request, &info).await?;
            let result = client.exec(request, &info).await;
            client.post(&result, &info).await?;
            let response = result?;
            match response.status().as_u16() {
                201u16 => Ok(ResponseValue::stream(response)),
                422u16 => Err(Error::ErrorResponse(ResponseValue::empty(response))),
                _ => Err(Error::UnexpectedResponse(response)),
            }
        }
    }
    /**Builder for [`Client::get_book`]

    [`Client::get_book`]: super::Client::get_book*/
    #[derive(Debug, Clone)]
    pub struct GetBook<'a> {
        client: &'a super::Client,
        id: Result<i32, String>,
    }
    impl<'a> GetBook<'a> {
        pub fn new(client: &'a super::Client) -> Self {
            Self {
                client: client,
                id: Err("id was not initialized".to_string()),
            }
        }
        pub fn id<V>(mut self, value: V) -> Self
        where
            V: std::convert::TryInto<i32>,
        {
            self.id = value
                .try_into()
                .map_err(|_| "conversion to `i32` for id failed".to_string());
            self
        }
        ///Sends a `GET` request to `/books/{id}`
        pub async fn send(self) -> Result<ResponseValue<types::Book>, Error<()>> {
            let Self { client, id } = self;
            let id = id.map_err(Error::InvalidRequest)?;
            let url = format!("{}/books/{}", client.baseurl, encode_path(&id.to_string()),);
            let mut header_map = ::reqwest::header::HeaderMap::with_capacity(1usize);
            header_map.append(
                ::reqwest::header::HeaderName::from_static("api-version"),
                ::reqwest::header::HeaderValue::from_static(super::Client::api_version()),
            );
            #[allow(unused_mut)]
            let mut request = client
                .client
                .get(url)
                .header(
                    ::reqwest::header::ACCEPT,
                    ::reqwest::header::HeaderValue::from_static("application/json"),
                )
                .headers(header_map)
                .build()?;
            let info = OperationInfo {
                operation_id: "get_book",
            };
            match (|_, request: &mut reqwest::Request| {
                crate::inject_opentelemetry_context_into_request(request);
                Box::pin(async { Ok::<_, Box<dyn std::error::Error>>(()) })
            })(&client.inner, &mut request)
            .await
            {
                Ok(_) => {}
                Err(e) => return Err(Error::Custom(e.to_string())),
            }
            client.pre(&mut request, &info).await?;
            let result = client.exec(request, &info).await;
            client.post(&result, &info).await?;
            let response = result?;
            match response.status().as_u16() {
                200u16 => ResponseValue::from_response(response).await,
                404u16 => Err(Error::ErrorResponse(ResponseValue::empty(response))),
                _ => Err(Error::UnexpectedResponse(response)),
            }
        }
    }
    /**Builder for [`Client::delete_book`]

    [`Client::delete_book`]: super::Client::delete_book*/
    #[derive(Debug, Clone)]
    pub struct DeleteBook<'a> {
        client: &'a super::Client,
        id: Result<i32, String>,
    }
    impl<'a> DeleteBook<'a> {
        pub fn new(client: &'a super::Client) -> Self {
            Self {
                client: client,
                id: Err("id was not initialized".to_string()),
            }
        }
        pub fn id<V>(mut self, value: V) -> Self
        where
            V: std::convert::TryInto<i32>,
        {
            self.id = value
                .try_into()
                .map_err(|_| "conversion to `i32` for id failed".to_string());
            self
        }
        ///Sends a `DELETE` request to `/books/{id}`
        pub async fn send(self) -> Result<ResponseValue<()>, Error<()>> {
            let Self { client, id } = self;
            let id = id.map_err(Error::InvalidRequest)?;
            let url = format!("{}/books/{}", client.baseurl, encode_path(&id.to_string()),);
            let mut header_map = ::reqwest::header::HeaderMap::with_capacity(1usize);
            header_map.append(
                ::reqwest::header::HeaderName::from_static("api-version"),
                ::reqwest::header::HeaderValue::from_static(super::Client::api_version()),
            );
            #[allow(unused_mut)]
            let mut request = client.client.delete(url).headers(header_map).build()?;
            let info = OperationInfo {
                operation_id: "delete_book",
            };
            match (|_, request: &mut reqwest::Request| {
                crate::inject_opentelemetry_context_into_request(request);
                Box::pin(async { Ok::<_, Box<dyn std::error::Error>>(()) })
            })(&client.inner, &mut request)
            .await
            {
                Ok(_) => {}
                Err(e) => return Err(Error::Custom(e.to_string())),
            }
            client.pre(&mut request, &info).await?;
            let result = client.exec(request, &info).await;
            client.post(&result, &info).await?;
            let response = result?;
            match response.status().as_u16() {
                200u16 => Ok(ResponseValue::empty(response)),
                404u16 => Err(Error::ErrorResponse(ResponseValue::empty(response))),
                _ => Err(Error::UnexpectedResponse(response)),
            }
        }
    }
    /**Builder for [`Client::update_book`]

    [`Client::update_book`]: super::Client::update_book*/
    #[derive(Debug, Clone)]
    pub struct UpdateBook<'a> {
        client: &'a super::Client,
        id: Result<i32, String>,
        body: Result<types::builder::BookCreateIn, String>,
    }
    impl<'a> UpdateBook<'a> {
        pub fn new(client: &'a super::Client) -> Self {
            Self {
                client: client,
                id: Err("id was not initialized".to_string()),
                body: Ok(::std::default::Default::default()),
            }
        }
        pub fn id<V>(mut self, value: V) -> Self
        where
            V: std::convert::TryInto<i32>,
        {
            self.id = value
                .try_into()
                .map_err(|_| "conversion to `i32` for id failed".to_string());
            self
        }
        pub fn body<V>(mut self, value: V) -> Self
        where
            V: std::convert::TryInto<types::BookCreateIn>,
            <V as std::convert::TryInto<types::BookCreateIn>>::Error: std::fmt::Display,
        {
            self.body = value
                .try_into()
                .map(From::from)
                .map_err(|s| format!("conversion to `BookCreateIn` for body failed: {s}"));
            self
        }
        pub fn body_map<F>(mut self, f: F) -> Self
        where
            F: std::ops::FnOnce(types::builder::BookCreateIn) -> types::builder::BookCreateIn,
        {
            self.body = self.body.map(f);
            self
        }
        ///Sends a `PATCH` request to `/books/{id}`
        pub async fn send(self) -> Result<ResponseValue<ByteStream>, Error<()>> {
            let Self { client, id, body } = self;
            let id = id.map_err(Error::InvalidRequest)?;
            let body = body
                .and_then(|v| types::BookCreateIn::try_from(v).map_err(|e| e.to_string()))
                .map_err(Error::InvalidRequest)?;
            let url = format!("{}/books/{}", client.baseurl, encode_path(&id.to_string()),);
            let mut header_map = ::reqwest::header::HeaderMap::with_capacity(1usize);
            header_map.append(
                ::reqwest::header::HeaderName::from_static("api-version"),
                ::reqwest::header::HeaderValue::from_static(super::Client::api_version()),
            );
            #[allow(unused_mut)]
            let mut request = client
                .client
                .patch(url)
                .json(&body)
                .headers(header_map)
                .build()?;
            let info = OperationInfo {
                operation_id: "update_book",
            };
            match (|_, request: &mut reqwest::Request| {
                crate::inject_opentelemetry_context_into_request(request);
                Box::pin(async { Ok::<_, Box<dyn std::error::Error>>(()) })
            })(&client.inner, &mut request)
            .await
            {
                Ok(_) => {}
                Err(e) => return Err(Error::Custom(e.to_string())),
            }
            client.pre(&mut request, &info).await?;
            let result = client.exec(request, &info).await;
            client.post(&result, &info).await?;
            let response = result?;
            match response.status().as_u16() {
                200u16 => Ok(ResponseValue::stream(response)),
                404u16 => Err(Error::ErrorResponse(ResponseValue::empty(response))),
                _ => Err(Error::UnexpectedResponse(response)),
            }
        }
    }
    /**Builder for [`Client::search_books`]

    [`Client::search_books`]: super::Client::search_books*/
    #[derive(Debug, Clone)]
    pub struct SearchBooks<'a> {
        client: &'a super::Client,
        limit: Result<Option<i64>, String>,
        q: Result<::std::string::String, String>,
    }
    impl<'a> SearchBooks<'a> {
        pub fn new(client: &'a super::Client) -> Self {
            Self {
                client: client,
                limit: Ok(None),
                q: Err("q was not initialized".to_string()),
            }
        }
        pub fn limit<V>(mut self, value: V) -> Self
        where
            V: std::convert::TryInto<i64>,
        {
            self.limit = value
                .try_into()
                .map(Some)
                .map_err(|_| "conversion to `i64` for limit failed".to_string());
            self
        }
        pub fn q<V>(mut self, value: V) -> Self
        where
            V: std::convert::TryInto<::std::string::String>,
        {
            self.q = value
                .try_into()
                .map_err(|_| "conversion to `:: std :: string :: String` for q failed".to_string());
            self
        }
        ///Sends a `GET` request to `/books/search`
        pub async fn send(
            self,
        ) -> Result<ResponseValue<::std::vec::Vec<types::BookSearchResult>>, Error<()>> {
            let Self { client, limit, q } = self;
            let limit = limit.map_err(Error::InvalidRequest)?;
            let q = q.map_err(Error::InvalidRequest)?;
            let url = format!("{}/books/search", client.baseurl,);
            let mut header_map = ::reqwest::header::HeaderMap::with_capacity(1usize);
            header_map.append(
                ::reqwest::header::HeaderName::from_static("api-version"),
                ::reqwest::header::HeaderValue::from_static(super::Client::api_version()),
            );
            #[allow(unused_mut)]
            let mut request = client
                .client
                .get(url)
                .header(
                    ::reqwest::header::ACCEPT,
                    ::reqwest::header::HeaderValue::from_static("application/json"),
                )
                .query(&progenitor_client::QueryParam::new("limit", &limit))
                .query(&progenitor_client::QueryParam::new("q", &q))
                .headers(header_map)
                .build()?;
            let info = OperationInfo {
                operation_id: "search_books",
            };
            match (|_, request: &mut reqwest::Request| {
                crate::inject_opentelemetry_context_into_request(request);
                Box::pin(async { Ok::<_, Box<dyn std::error::Error>>(()) })
            })(&client.inner, &mut request)
            .await
            {
                Ok(_) => {}
                Err(e) => return Err(Error::Custom(e.to_string())),
            }
            client.pre(&mut request, &info).await?;
            let result = client.exec(request, &info).await;
            client.post(&result, &info).await?;
            let response = result?;
            match response.status().as_u16() {
                200u16 => ResponseValue::from_response(response).await,
                _ => Err(Error::UnexpectedResponse(response)),
            }
        }
    }
}
/// Items consumers will typically use such as the Client.
pub mod prelude {
    pub use self::super::Client;
}
