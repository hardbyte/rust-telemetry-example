import json
import string

from locust import HttpUser, TaskSet, task, between
import random

class BookTasks(TaskSet):

    def on_start(self) -> None:
        self.created_book_ids = []


    @task(100)
    def get_book(self):
        # Randomly select a book ID
        book_id = random.randint(1, 90)
        # Define the endpoint URL
        url = f"/books/{book_id}"
        # Make the GET request with the Accept header
        with self.client.get(url, headers={"Accept": "application/json"}, catch_response=True) as response:
            if response.status_code != 200:
                response.failure(f"Failed to retrieve book with ID {book_id}")
            else:
                response.success()

    @task(1)
    def get_many_books(self):
        # Define the endpoint URL
        url = "/books"
        # Make the GET request with the Accept header
        with self.client.get(url, headers={"Accept": "application/json"}, catch_response=True) as response:
            if response.status_code != 200:
                response.failure(f"Failed to retrieve many book")
            else:
                response.success()

    @task(2)  # Weight of 2 for POST requests
    def create_book(self):
        """Task to create a new book with random title and author."""
        # Generate random title and author
        title = "Book " + ''.join(random.choices(string.ascii_letters + string.digits, k=8))
        author = "Author " + ''.join(random.choices(string.ascii_letters + string.digits, k=5))
        payload = {
            "title": title,
            "author": author
        }
        url = "/books/add"
        headers = {"Content-Type": "application/json"}
        with self.client.post(url, data=json.dumps(payload), headers=headers, catch_response=True) as response:
            if response.status_code == 200 or response.status_code == 201:
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

    @task(3)  # Weight of 3 for DELETE requests
    def delete_book(self):
        """Task to delete a previously created book."""
        if self.created_book_ids:
            # Randomly select a book ID from the list of created books
            book_id = random.choice(self.created_book_ids)
            url = f"/books/{book_id}"
            with self.client.delete(url, catch_response=True) as response:
                if response.status_code == 200 or response.status_code == 204:
                    # Remove the ID from the list as it's deleted
                    self.created_book_ids.remove(book_id)
                    response.success()
                else:
                    response.failure(f"Failed to delete book with ID {book_id}: {response.text}")
        else:
            # If no books have been created yet, skip deletion
            pass

class BookUser(HttpUser):
    # Assign the task set to the user
    tasks = [BookTasks]
    # Wait time between tasks (1 to 5 seconds)
    wait_time = between(1, 5)
    # Set the host to the API's base URL
    host = "http://localhost:8000"

    def on_start(self):
        """Executed when a simulated user starts."""
        pass  # You can add any initialization logic here if needed

    def on_stop(self):
        """Executed when a simulated user stops."""
        pass  # You can add any teardown logic here if needed
