#![allow(clippy::all)]
#[allow(unused_imports)]
use progenitor_client::{encode_path, RequestBuilderExt};
#[allow(unused_imports)]
pub use progenitor_client::{ByteStream, Error, ResponseValue};
#[allow(unused_imports)]
use reqwest::header::{HeaderMap, HeaderValue};
/// Types used as operation parameters and responses.
#[allow(clippy::all)]
pub mod types {
    /// Error types.
    pub mod error {
        /// Error from a TryFrom or FromStr implementation.
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
    ///Book
    ///
    /// <details><summary>JSON schema</summary>
    ///
    /// ```json
    ///{
    ///  "type": "object",
    ///  "properties": {
    ///    "author": {
    ///      "examples": [
    ///        "George Orwell"
    ///      ],
    ///      "type": "string"
    ///    },
    ///    "id": {
    ///      "examples": [
    ///        1
    ///      ],
    ///      "type": "integer"
    ///    },
    ///    "title": {
    ///      "examples": [
    ///        "1984"
    ///      ],
    ///      "type": "string"
    ///    }
    ///  }
    ///}
    /// ```
    /// </details>
    #[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
    pub struct Book {
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        pub author: ::std::option::Option<::std::string::String>,
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        pub id: ::std::option::Option<i64>,
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        pub title: ::std::option::Option<::std::string::String>,
    }
    impl ::std::convert::From<&Book> for Book {
        fn from(value: &Book) -> Self {
            value.clone()
        }
    }
    impl ::std::default::Default for Book {
        fn default() -> Self {
            Self {
                author: Default::default(),
                id: Default::default(),
                title: Default::default(),
            }
        }
    }
    impl Book {
        pub fn builder() -> builder::Book {
            Default::default()
        }
    }
    ///BookCreateIn
    ///
    /// <details><summary>JSON schema</summary>
    ///
    /// ```json
    ///{
    ///  "type": "object",
    ///  "properties": {
    ///    "author": {
    ///      "examples": [
    ///        "George Orwell"
    ///      ],
    ///      "type": "string"
    ///    },
    ///    "title": {
    ///      "examples": [
    ///        "1984"
    ///      ],
    ///      "type": "string"
    ///    }
    ///  }
    ///}
    /// ```
    /// </details>
    #[derive(::serde::Deserialize, ::serde::Serialize, Clone, Debug)]
    pub struct BookCreateIn {
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        pub author: ::std::option::Option<::std::string::String>,
        #[serde(default, skip_serializing_if = "::std::option::Option::is_none")]
        pub title: ::std::option::Option<::std::string::String>,
    }
    impl ::std::convert::From<&BookCreateIn> for BookCreateIn {
        fn from(value: &BookCreateIn) -> Self {
            value.clone()
        }
    }
    impl ::std::default::Default for BookCreateIn {
        fn default() -> Self {
            Self {
                author: Default::default(),
                title: Default::default(),
            }
        }
    }
    impl BookCreateIn {
        pub fn builder() -> builder::BookCreateIn {
            Default::default()
        }
    }
    /// Types for composing complex structures.
    pub mod builder {
        #[derive(Clone, Debug)]
        pub struct Book {
            author: ::std::result::Result<
                ::std::option::Option<::std::string::String>,
                ::std::string::String,
            >,
            id: ::std::result::Result<::std::option::Option<i64>, ::std::string::String>,
            title: ::std::result::Result<
                ::std::option::Option<::std::string::String>,
                ::std::string::String,
            >,
        }
        impl ::std::default::Default for Book {
            fn default() -> Self {
                Self {
                    author: Ok(Default::default()),
                    id: Ok(Default::default()),
                    title: Ok(Default::default()),
                }
            }
        }
        impl Book {
            pub fn author<T>(mut self, value: T) -> Self
            where
                T: ::std::convert::TryInto<::std::option::Option<::std::string::String>>,
                T::Error: ::std::fmt::Display,
            {
                self.author = value
                    .try_into()
                    .map_err(|e| format!("error converting supplied value for author: {}", e));
                self
            }
            pub fn id<T>(mut self, value: T) -> Self
            where
                T: ::std::convert::TryInto<::std::option::Option<i64>>,
                T::Error: ::std::fmt::Display,
            {
                self.id = value
                    .try_into()
                    .map_err(|e| format!("error converting supplied value for id: {}", e));
                self
            }
            pub fn title<T>(mut self, value: T) -> Self
            where
                T: ::std::convert::TryInto<::std::option::Option<::std::string::String>>,
                T::Error: ::std::fmt::Display,
            {
                self.title = value
                    .try_into()
                    .map_err(|e| format!("error converting supplied value for title: {}", e));
                self
            }
        }
        impl ::std::convert::TryFrom<Book> for super::Book {
            type Error = super::error::ConversionError;
            fn try_from(value: Book) -> ::std::result::Result<Self, super::error::ConversionError> {
                Ok(Self {
                    author: value.author?,
                    id: value.id?,
                    title: value.title?,
                })
            }
        }
        impl ::std::convert::From<super::Book> for Book {
            fn from(value: super::Book) -> Self {
                Self {
                    author: Ok(value.author),
                    id: Ok(value.id),
                    title: Ok(value.title),
                }
            }
        }
        #[derive(Clone, Debug)]
        pub struct BookCreateIn {
            author: ::std::result::Result<
                ::std::option::Option<::std::string::String>,
                ::std::string::String,
            >,
            title: ::std::result::Result<
                ::std::option::Option<::std::string::String>,
                ::std::string::String,
            >,
        }
        impl ::std::default::Default for BookCreateIn {
            fn default() -> Self {
                Self {
                    author: Ok(Default::default()),
                    title: Ok(Default::default()),
                }
            }
        }
        impl BookCreateIn {
            pub fn author<T>(mut self, value: T) -> Self
            where
                T: ::std::convert::TryInto<::std::option::Option<::std::string::String>>,
                T::Error: ::std::fmt::Display,
            {
                self.author = value
                    .try_into()
                    .map_err(|e| format!("error converting supplied value for author: {}", e));
                self
            }
            pub fn title<T>(mut self, value: T) -> Self
            where
                T: ::std::convert::TryInto<::std::option::Option<::std::string::String>>,
                T::Error: ::std::fmt::Display,
            {
                self.title = value
                    .try_into()
                    .map_err(|e| format!("error converting supplied value for title: {}", e));
                self
            }
        }
        impl ::std::convert::TryFrom<BookCreateIn> for super::BookCreateIn {
            type Error = super::error::ConversionError;
            fn try_from(
                value: BookCreateIn,
            ) -> ::std::result::Result<Self, super::error::ConversionError> {
                Ok(Self {
                    author: value.author?,
                    title: value.title?,
                })
            }
        }
        impl ::std::convert::From<super::BookCreateIn> for BookCreateIn {
            fn from(value: super::BookCreateIn) -> Self {
                Self {
                    author: Ok(value.author),
                    title: Ok(value.title),
                }
            }
        }
    }
}
#[derive(Clone, Debug)]
/**Client for Book Service API

API for managing books in the library.

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
    /// Get the base URL to which requests are made.
    pub fn baseurl(&self) -> &String {
        &self.baseurl
    }
    /// Get the internal `reqwest::Client` used to make requests.
    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }
    /// Get the version of this API.
    ///
    /// This string is pulled directly from the source OpenAPI
    /// document and may be in any format the API selects.
    pub fn api_version(&self) -> &'static str {
        "1.0.0"
    }
    /// Return a reference to the inner type stored in `self`.
    pub fn inner(&self) -> &crate::ClientState {
        &self.inner
    }
}
impl Client {
    /**Get all books

    Sends a `GET` request to `/books/`

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
}
/// Types for composing operation parameters.
#[allow(clippy::all)]
pub mod builder {
    use super::types;
    #[allow(unused_imports)]
    use super::{
        encode_path, ByteStream, Error, HeaderMap, HeaderValue, RequestBuilderExt, ResponseValue,
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
        ///Sends a `GET` request to `/books/`
        pub async fn send(self) -> Result<ResponseValue<::std::vec::Vec<types::Book>>, Error<()>> {
            let Self { client } = self;
            let url = format!("{}/books/", client.baseurl,);
            #[allow(unused_mut)]
            let mut request = client
                .client
                .get(url)
                .header(
                    reqwest::header::ACCEPT,
                    reqwest::header::HeaderValue::from_static("application/json"),
                )
                .build()?;
            match (|_, request: &mut reqwest::Request| {
                crate::inject_opentelemetry_context_into_request(request);
                Box::pin(async { Ok::<_, Box<dyn std::error::Error>>(()) })
            })(&client.inner, &mut request)
            .await
            {
                Ok(_) => {}
                Err(e) => return Err(Error::PreHookError(e.to_string())),
            }
            let result = client.client.execute(request).await;
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
                .map_err(|s| format!("conversion to `BookCreateIn` for body failed: {}", s));
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
        pub async fn send(self) -> Result<ResponseValue<i64>, Error<()>> {
            let Self { client, body } = self;
            let body = body
                .and_then(|v| types::BookCreateIn::try_from(v).map_err(|e| e.to_string()))
                .map_err(Error::InvalidRequest)?;
            let url = format!("{}/books/add", client.baseurl,);
            #[allow(unused_mut)]
            let mut request = client
                .client
                .post(url)
                .header(
                    reqwest::header::ACCEPT,
                    reqwest::header::HeaderValue::from_static("application/json"),
                )
                .json(&body)
                .build()?;
            match (|_, request: &mut reqwest::Request| {
                crate::inject_opentelemetry_context_into_request(request);
                Box::pin(async { Ok::<_, Box<dyn std::error::Error>>(()) })
            })(&client.inner, &mut request)
            .await
            {
                Ok(_) => {}
                Err(e) => return Err(Error::PreHookError(e.to_string())),
            }
            let result = client.client.execute(request).await;
            let response = result?;
            match response.status().as_u16() {
                200u16 => ResponseValue::from_response(response).await,
                404u16 => Err(Error::ErrorResponse(ResponseValue::empty(response))),
                _ => Err(Error::UnexpectedResponse(response)),
            }
        }
    }
    /**Builder for [`Client::get_book`]

    [`Client::get_book`]: super::Client::get_book*/
    #[derive(Debug, Clone)]
    pub struct GetBook<'a> {
        client: &'a super::Client,
        id: Result<i64, String>,
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
            V: std::convert::TryInto<i64>,
        {
            self.id = value
                .try_into()
                .map_err(|_| "conversion to `i64` for id failed".to_string());
            self
        }
        ///Sends a `GET` request to `/books/{id}`
        pub async fn send(self) -> Result<ResponseValue<types::Book>, Error<()>> {
            let Self { client, id } = self;
            let id = id.map_err(Error::InvalidRequest)?;
            let url = format!("{}/books/{}", client.baseurl, encode_path(&id.to_string()),);
            #[allow(unused_mut)]
            let mut request = client
                .client
                .get(url)
                .header(
                    reqwest::header::ACCEPT,
                    reqwest::header::HeaderValue::from_static("application/json"),
                )
                .build()?;
            match (|_, request: &mut reqwest::Request| {
                crate::inject_opentelemetry_context_into_request(request);
                Box::pin(async { Ok::<_, Box<dyn std::error::Error>>(()) })
            })(&client.inner, &mut request)
            .await
            {
                Ok(_) => {}
                Err(e) => return Err(Error::PreHookError(e.to_string())),
            }
            let result = client.client.execute(request).await;
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
        id: Result<i64, String>,
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
            V: std::convert::TryInto<i64>,
        {
            self.id = value
                .try_into()
                .map_err(|_| "conversion to `i64` for id failed".to_string());
            self
        }
        ///Sends a `DELETE` request to `/books/{id}`
        pub async fn send(self) -> Result<ResponseValue<()>, Error<()>> {
            let Self { client, id } = self;
            let id = id.map_err(Error::InvalidRequest)?;
            let url = format!("{}/books/{}", client.baseurl, encode_path(&id.to_string()),);
            #[allow(unused_mut)]
            let mut request = client.client.delete(url).build()?;
            match (|_, request: &mut reqwest::Request| {
                crate::inject_opentelemetry_context_into_request(request);
                Box::pin(async { Ok::<_, Box<dyn std::error::Error>>(()) })
            })(&client.inner, &mut request)
            .await
            {
                Ok(_) => {}
                Err(e) => return Err(Error::PreHookError(e.to_string())),
            }
            let result = client.client.execute(request).await;
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
        id: Result<i64, String>,
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
            V: std::convert::TryInto<i64>,
        {
            self.id = value
                .try_into()
                .map_err(|_| "conversion to `i64` for id failed".to_string());
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
                .map_err(|s| format!("conversion to `BookCreateIn` for body failed: {}", s));
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
        pub async fn send(self) -> Result<ResponseValue<i64>, Error<()>> {
            let Self { client, id, body } = self;
            let id = id.map_err(Error::InvalidRequest)?;
            let body = body
                .and_then(|v| types::BookCreateIn::try_from(v).map_err(|e| e.to_string()))
                .map_err(Error::InvalidRequest)?;
            let url = format!("{}/books/{}", client.baseurl, encode_path(&id.to_string()),);
            #[allow(unused_mut)]
            let mut request = client
                .client
                .patch(url)
                .header(
                    reqwest::header::ACCEPT,
                    reqwest::header::HeaderValue::from_static("application/json"),
                )
                .json(&body)
                .build()?;
            match (|_, request: &mut reqwest::Request| {
                crate::inject_opentelemetry_context_into_request(request);
                Box::pin(async { Ok::<_, Box<dyn std::error::Error>>(()) })
            })(&client.inner, &mut request)
            .await
            {
                Ok(_) => {}
                Err(e) => return Err(Error::PreHookError(e.to_string())),
            }
            let result = client.client.execute(request).await;
            let response = result?;
            match response.status().as_u16() {
                200u16 => ResponseValue::from_response(response).await,
                404u16 => Err(Error::ErrorResponse(ResponseValue::empty(response))),
                _ => Err(Error::UnexpectedResponse(response)),
            }
        }
    }
}
/// Items consumers will typically use such as the Client.
pub mod prelude {
    pub use self::super::Client;
}
