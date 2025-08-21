# /// script
# requires-python = ">=3.12"
# dependencies = [
#     "locust",
#     "opentelemetry-sdk >1.24",
#     "opentelemetry-exporter-otlp-proto-grpc >=1.24.0",
#    opentelemetry-instrumentation-requests==0.46b0
#    opentelemetry-instrumentation-system-metrics==0.46b0
#    "opentelemetry-instrumentation-urllib3==0.46b0",
# ]
# ///
from locust import HttpUser, TaskSet, task, between

try:
    from opentelemetry import trace
    from opentelemetry.exporter.otlp.proto.grpc.trace_exporter import OTLPSpanExporter
    from opentelemetry.sdk.resources import SERVICE_NAME, Resource
    from opentelemetry.sdk.trace import TracerProvider, ReadableSpan
    from opentelemetry.instrumentation.requests import RequestsInstrumentor
    # from opentelemetry.instrumentation.system_metrics import SystemMetricsInstrumentor
    from opentelemetry.instrumentation.urllib3 import URLLib3Instrumentor
    from opentelemetry.sdk.trace.export import BatchSpanProcessor

except ImportError:
    print("opentelemetry is not installed. Tracing will be disabled.")
    trace = None

import os
import json
import string
import random

def init_telemetry(
        service_name: str = "load-tester-client"
):
    if trace is None:
        return

    # Allow service name override from environment
    service_name = os.environ.get("OTEL_SERVICE_NAME", os.environ.get("LOCUST_SERVICE_NAME", service_name))

    resource = Resource.create(
        {SERVICE_NAME: service_name}
    )
    provider = TracerProvider(resource=resource)

    endpoint = os.environ.get("OTEL_EXPORTER_OTLP_ENDPOINT", "http://localhost:4317")
    insecure_env = os.environ.get("OTEL_EXPORTER_OTLP_INSECURE")
    if insecure_env is not None:
        insecure = insecure_env.lower() in ("1", "true", "t", "yes", "y")
    else:
        insecure = endpoint.startswith("http://")
    span_exporter = OTLPSpanExporter(endpoint=endpoint, insecure=insecure)
    span_processor = BatchSpanProcessor(span_exporter)
    # add to the tracer
    provider.add_span_processor(span_processor)

    trace.set_tracer_provider(provider)

    # Configure any instruments
    RequestsInstrumentor().instrument()
    # SystemMetricsInstrumentor().instrument()
    URLLib3Instrumentor().instrument()

    tracer = trace.get_tracer(__name__)
    with tracer.start_as_current_span("load balancer test span"):
        print("tracing enabled")


try:
    init_telemetry(os.environ.get('OTEL_SERVICE_NAME', os.environ.get('LOCUST_SERVICE_NAME', 'load-tester')))
except Exception as e:
    print(f"Failed to initialize telemetry: {e}")

class BookTasks(TaskSet):

    def on_start(self) -> None:
        self.created_book_ids = []


    @task(100)
    def get_book(self):
        # Randomly select a book ID
        book_id = random.randint(1, 90)
        # Define the endpoint URL
        url = f"/books/{book_id}"
        
        # Create a named span for better tracing
        if trace is not None:
            tracer = trace.get_tracer(__name__)
            with tracer.start_as_current_span("get_book") as span:
                span.set_attribute("book.id", book_id)
                span.set_attribute("http.url", url)
                span.set_attribute("operation.type", "get_book")
                
                # Make the GET request with the Accept header
                with self.client.get(url, headers={"Accept": "application/json"}, catch_response=True) as response:
                    span.set_attribute("http.status_code", response.status_code)
                    if response.status_code != 200:
                        span.set_attribute("error", True)
                        response.failure(f"Failed to retrieve book with ID {book_id}")
                    else:
                        response.success()
        else:
            # Fallback without tracing
            with self.client.get(url, headers={"Accept": "application/json"}, catch_response=True) as response:
                if response.status_code != 200:
                    response.failure(f"Failed to retrieve book with ID {book_id}")
                else:
                    response.success()

    @task(1)
    def get_many_books(self):
        # Define the endpoint URL
        url = "/books"
        
        # Create a named span for better tracing
        if trace is not None:
            tracer = trace.get_tracer(__name__)
            with tracer.start_as_current_span("get_all_books") as span:
                span.set_attribute("http.url", url)
                span.set_attribute("operation.type", "get_all_books")
                
                # Make the GET request with the Accept header
                with self.client.get(url, headers={"Accept": "application/json"}, catch_response=True) as response:
                    span.set_attribute("http.status_code", response.status_code)
                    if response.status_code == 200:
                        try:
                            books = response.json()
                            if isinstance(books, list):
                                span.set_attribute("books.count", len(books))
                                response.success()
                            else:
                                span.set_attribute("error", True)
                                response.failure("Books response is not a list")
                        except Exception:
                            span.set_attribute("error", True)
                            response.failure("Failed to decode books response JSON")
                    else:
                        span.set_attribute("error", True)
                        response.failure(f"Failed to retrieve many books")
        else:
            # Fallback without tracing
            with self.client.get(url, headers={"Accept": "application/json"}, catch_response=True) as response:
                if response.status_code != 200:
                    response.failure(f"Failed to retrieve many books")
                else:
                    response.success()

    @task(2)  # Weight of 2 for POST requests
    def create_book(self):
        """Task to create a new book with random title and author."""
        # Generate random title and author
        title = "Book " + ''.join(random.choices(string.ascii_letters + string.digits, k=8))
        author = "Author " + ''.join(random.choices(string.ascii_letters + string.digits, k=5))
        payload = {
            "work_title": title,
            "primary_author_name": author
        }
        has_extra_data = random.random() > 0.5
        if has_extra_data:
            payload["extra-data"] = random.randbytes(1000).hex()
        url = "/books/add"
        
        # Create a named span for better tracing
        if trace is not None:
            tracer = trace.get_tracer(__name__)
            with tracer.start_as_current_span("create_book") as span:
                span.set_attribute("book.title", title)
                span.set_attribute("book.author", author)
                span.set_attribute("book.has_extra_data", has_extra_data)
                span.set_attribute("http.url", url)
                span.set_attribute("operation.type", "create_book")
                
                with self.client.post(url, json=payload, catch_response=True) as response:
                    span.set_attribute("http.status_code", response.status_code)
                    if response.status_code in (200, 201):
                        # Assuming the API returns the created book's ID in the response JSON
                        try:
                            response_data = response.json()
                            book_id = response_data
                            if book_id:
                                span.set_attribute("book.created_id", book_id)
                                self.created_book_ids.append(book_id)
                                response.success()
                            else:
                                span.set_attribute("error", True)
                                response.failure("No ID returned in response")
                        except json.JSONDecodeError:
                            span.set_attribute("error", True)
                            response.failure("Failed to decode JSON response")
                    else:
                        span.set_attribute("error", True)
                        response.failure(f"Failed to create book: {response.text}")
        else:
            # Fallback without tracing
            with self.client.post(url, json=payload, catch_response=True) as response:
                if response.status_code in (200, 201):
                    # Assuming the API returns the created book's ID in the response JSON
                    try:
                        response_data = response.json()
                        book_id = response_data
                        if book_id:
                            self.created_book_ids.append(book_id)
                            response.success()
                        else:
                            response.failure("No ID returned in response")
                    except json.JSONDecodeError:
                        response.failure("Failed to decode JSON response")
                else:
                    response.failure(f"Failed to create book: {response.text}")

    @task(1)
    def bulk_create_books(self):
        # generate 5–20 random books
        batch_size = random.randint(5, 20)
        payload = []
        for _ in range(batch_size):
            payload.append({
                "work_title": "Book " + ''.join(random.choices(string.ascii_letters + string.digits, k=6)),
                "primary_author_name": "Author " + ''.join(random.choices(string.ascii_letters + string.digits, k=4))
            })
        
        url = "/books/bulk_add"
        
        # Create a named span for better tracing
        if trace is not None:
            tracer = trace.get_tracer(__name__)
            with tracer.start_as_current_span("bulk_create_books") as span:
                span.set_attribute("books.batch_size", batch_size)
                span.set_attribute("http.url", url)
                span.set_attribute("operation.type", "bulk_create_books")
                
                with self.client.post(url, json=payload, catch_response=True) as response:
                    span.set_attribute("http.status_code", response.status_code)
                    if response.status_code in (200, 201):
                        try:
                            ids = response.json()
                            if isinstance(ids, list):
                                span.set_attribute("books.created_count", len(ids))
                                self.created_book_ids.extend(ids)
                                response.success()
                            else:
                                span.set_attribute("error", True)
                                response.failure("Unexpected payload shape from bulk_add")
                        except Exception:
                            span.set_attribute("error", True)
                            response.failure("Failed to decode JSON response for bulk create")
                    else:
                        span.set_attribute("error", True)
                        response.failure(f"Bulk create failed: {response.text}")
        else:
            # Fallback without tracing
            with self.client.post(url, json=payload, catch_response=True) as response:
                if response.status_code in (200, 201):
                    try:
                        ids = response.json()
                        if isinstance(ids, list):
                            self.created_book_ids.extend(ids)
                            response.success()
                        else:
                            response.failure("Unexpected payload shape from bulk_add")
                    except Exception:
                        response.failure("Failed to decode JSON response for bulk create")
                else:
                    response.failure(f"Bulk create failed: {response.text}")

    @task(3)  # Weight of 3 for DELETE requests
    def delete_book(self):
        """Task to delete a previously created book."""
        if self.created_book_ids:
            # Randomly select a book ID from the list of created books
            book_id = random.choice(self.created_book_ids)
            url = f"/books/{book_id}"
            
            # Create a named span for better tracing
            if trace is not None:
                tracer = trace.get_tracer(__name__)
                with tracer.start_as_current_span("delete_book") as span:
                    span.set_attribute("book.id", book_id)
                    span.set_attribute("http.url", url)
                    span.set_attribute("operation.type", "delete_book")
                    
                    with self.client.delete(url, catch_response=True) as response:
                        span.set_attribute("http.status_code", response.status_code)
                        if response.status_code in (200, 204):
                            # Remove the ID from the list as it's deleted
                            self.created_book_ids.remove(book_id)
                            span.set_attribute("book.deleted", True)
                            response.success()
                        else:
                            span.set_attribute("error", True)
                            response.failure(f"Failed to delete book with ID {book_id}: {response.text}")
            else:
                # Fallback without tracing
                with self.client.delete(url, catch_response=True) as response:
                    if response.status_code in (200, 204):
                        # Remove the ID from the list as it's deleted
                        self.created_book_ids.remove(book_id)
                        response.success()
                    else:
                        response.failure(f"Failed to delete book with ID {book_id}: {response.text}")
        else:
            # If no books have been created yet, skip deletion
            pass

    @task(10)  # Weight of 10 for search requests - common operation
    def search_books(self):
        """Task to search for books using full-text search."""
        # Random search queries that should match created books
        search_terms = [
            "Book",
            "Author", 
            "Test",
            "Fantasy",
            "Science",
            "Fiction",
            "Harry",
            "Potter",
            "Tolkien",
            "Martin",
            "Random",
            # Single letters for broader matches
            "A", "B", "C", "S", "T"
        ]
        
        query = random.choice(search_terms)
        limit = random.randint(5, 50)
        url = f"/books/search?q={query}&limit={limit}"
        
        # Create a named span for better search tracing
        if trace is not None:
            tracer = trace.get_tracer(__name__)
            with tracer.start_as_current_span(f"search_books") as span:
                span.set_attribute("search.query", query)
                span.set_attribute("search.limit", limit)
                span.set_attribute("http.url", url)
                span.set_attribute("operation.type", "search_books")
                
                with self.client.get(url, headers={"Accept": "application/json"}, catch_response=True) as response:
                    span.set_attribute("http.status_code", response.status_code)
                    if response.status_code == 200:
                        try:
                            results = response.json()
                            if isinstance(results, list):
                                span.set_attribute("search.results_count", len(results))
                                response.success()
                            else:
                                span.set_attribute("error", True)
                                response.failure("Search response is not a list")
                        except json.JSONDecodeError:
                            span.set_attribute("error", True)
                            response.failure("Failed to decode search response JSON")
                    elif response.status_code == 400:
                        # Bad request (empty query, etc.) - this is expected for some edge cases
                        span.set_attribute("search.bad_request", True)
                        response.success()
                    else:
                        span.set_attribute("error", True)
                        response.failure(f"Search failed with status {response.status_code}: {response.text}")
        else:
            # Fallback without tracing
            with self.client.get(url, headers={"Accept": "application/json"}, catch_response=True) as response:
                if response.status_code == 200:
                    try:
                        results = response.json()
                        if isinstance(results, list):
                            response.success()
                        else:
                            response.failure("Search response is not a list")
                    except json.JSONDecodeError:
                        response.failure("Failed to decode search response JSON")
                elif response.status_code == 400:
                    # Bad request (empty query, etc.) - this is expected for some edge cases
                    response.success()
                else:
                    response.failure(f"Search failed with status {response.status_code}: {response.text}")

    @task(5)  # Weight of 5 for specific searches
    def search_books_specific(self):
        """Task to search for books with more specific queries."""
        # More targeted search queries
        specific_queries = [
            "Book AND Author",
            "Fantasy OR Science",
            "Harry Potter",
            "Lord of the Rings", 
            "Game of Thrones",
            "Tolkien",
            "Rowling",
            "Martin"
        ]
        
        query = random.choice(specific_queries)
        limit = random.randint(1, 20)
        url = f"/books/search?q={query}&limit={limit}"
        
        # Create a named span for specific search tracing
        if trace is not None:
            tracer = trace.get_tracer(__name__)
            with tracer.start_as_current_span(f"search_books_specific") as span:
                span.set_attribute("search.query", query)
                span.set_attribute("search.limit", limit)
                span.set_attribute("search.type", "specific")
                span.set_attribute("http.url", url)
                span.set_attribute("operation.type", "search_books_specific")
                
                with self.client.get(url, headers={"Accept": "application/json"}, catch_response=True) as response:
                    span.set_attribute("http.status_code", response.status_code)
                    if response.status_code == 200:
                        try:
                            results = response.json()
                            if isinstance(results, list):
                                span.set_attribute("search.results_count", len(results))
                                response.success()
                            else:
                                span.set_attribute("error", True)
                                response.failure("Specific search response is not a list")
                        except json.JSONDecodeError:
                            span.set_attribute("error", True)
                            response.failure("Failed to decode specific search response JSON")
                    elif response.status_code == 400:
                        # Bad request - acceptable for some complex queries
                        span.set_attribute("search.complex_query", True)
                        response.success()
                    else:
                        span.set_attribute("error", True)
                        response.failure(f"Specific search failed with status {response.status_code}: {response.text}")
        else:
            # Fallback without tracing
            with self.client.get(url, headers={"Accept": "application/json"}, catch_response=True) as response:
                if response.status_code == 200:
                    try:
                        results = response.json()
                        if isinstance(results, list):
                            response.success()
                        else:
                            response.failure("Specific search response is not a list")
                    except json.JSONDecodeError:
                        response.failure("Failed to decode specific search response JSON")
                elif response.status_code == 400:
                    # Bad request - acceptable for some complex queries
                    response.success()
                else:
                    response.failure(f"Specific search failed with status {response.status_code}: {response.text}")

class BookUser(HttpUser):
    # Assign the task set to the user
    tasks = [BookTasks]
    # Wait time between tasks (1 to 5 seconds)
    wait_time = between(1, 5)
    # Set the host to the API's base URL
    host = os.environ.get("LOCUST_HOST", "http://localhost:8000")

    def on_start(self):
        """Executed when a simulated user starts."""
        pass  # You can add any initialization logic here if needed

    def on_stop(self):
        """Executed when a simulated user stops."""
        pass  # You can add any teardown logic here if needed
