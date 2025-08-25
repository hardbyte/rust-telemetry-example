"""
High-throughput load test for Rust microservices
Targets 500-1000 RPS sustained throughput
"""

import random
from locust import HttpUser, task, between
from opentelemetry import trace
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import BatchSpanProcessor
from opentelemetry.exporter.otlp.proto.grpc.trace_exporter import OTLPSpanExporter
from opentelemetry.instrumentation.requests import RequestsInstrumentor

# Configure OpenTelemetry
trace.set_tracer_provider(TracerProvider())
tracer = trace.get_tracer("locust-load-test")

# Configure OTLP exporter
otlp_exporter = OTLPSpanExporter(
    endpoint="http://localhost:4317",
    insecure=True,
)

# Add the span processor to the tracer provider
span_processor = BatchSpanProcessor(otlp_exporter)
trace.get_tracer_provider().add_span_processor(span_processor)

# Instrument requests library
RequestsInstrumentor().instrument()

class HighThroughputUser(HttpUser):
    # Aggressive timing - minimal wait between requests
    wait_time = between(0.01, 0.1)  # 10ms to 100ms between requests
    
    def on_start(self):
        """Initialize user session"""
        pass
    
    @task(40)  # Heavily weighted for read operations
    def get_individual_book(self):
        """Get individual book by ID - should be very fast"""
        book_id = random.randint(1, 100)
        with tracer.start_as_current_span(f"get_book_{book_id}"):
            self.client.get(f"/books/{book_id}")
    
    @task(20)  # Medium weight for all books
    def get_all_books(self):
        """Get all books - this was our previously slow endpoint"""
        with tracer.start_as_current_span("get_all_books"):
            self.client.get("/books")
    
    @task(15)  # Search operations
    def search_books(self):
        """Search books - test full-text search performance"""
        queries = ["Book", "Science", "Fantasy", "History", "Technology", "Adventure"]
        query = random.choice(queries)
        limit = random.randint(5, 50)
        with tracer.start_as_current_span(f"search_books_{query}"):
            self.client.get(f"/books/search?q={query}&limit={limit}")
    
    @task(10)  # Write operations
    def create_book(self):
        """Create new book - test write performance"""
        book_data = {
            "work_title": f"Test Book {random.randint(10000, 99999)}",
            "primary_author_name": f"Author {random.randint(1000, 9999)}",
            "status": random.choice(["ToRead", "Reading", "Read"])
        }
        with tracer.start_as_current_span("create_book"):
            self.client.post("/books/add", json=book_data)
    
    @task(8)  # Bulk operations
    def bulk_create_books(self):
        """Bulk create books - test batch performance"""
        books = []
        batch_size = random.randint(3, 10)  # Smaller batches for higher frequency
        for i in range(batch_size):
            books.append({
                "work_title": f"Bulk Book {random.randint(10000, 99999)}",
                "primary_author_name": f"Bulk Author {random.randint(1000, 9999)}",
                "status": random.choice(["ToRead", "Reading", "Read"])
            })
        with tracer.start_as_current_span(f"bulk_create_{batch_size}"):
            self.client.post("/books/bulk_add", json=books)
    
    @task(5)  # Update operations
    def update_book(self):
        """Update existing book"""
        book_id = random.randint(1, 100)
        update_data = {
            "work_title": f"Updated Book {random.randint(10000, 99999)}",
            "primary_author_name": f"Updated Author {random.randint(1000, 9999)}",
            "status": random.choice(["ToRead", "Reading", "Read"])
        }
        with tracer.start_as_current_span(f"update_book_{book_id}"):
            self.client.patch(f"/books/{book_id}", json=update_data)
    
    @task(2)  # Delete operations (low frequency)
    def delete_book(self):
        """Delete book - test delete performance"""
        book_id = random.randint(50, 150)  # Delete from a range that might not exist
        with tracer.start_as_current_span(f"delete_book_{book_id}"):
            self.client.delete(f"/books/{book_id}")

    @task(3)  # Health checks
    def health_check(self):
        """Health endpoint - should be instantaneous"""
        with tracer.start_as_current_span("health_check"):
            self.client.get("/health")