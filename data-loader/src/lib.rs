use client::types::{BookCreateIn, BookCreateInStatus};
use rand::prelude::IndexedRandom;

#[derive(Debug, Clone)]
pub struct BookCreateData {
    pub title: String,
    pub author: String,
}

const SAMPLE_AUTHORS: &[&str] = &[
    "Jane Austen",
    "Charles Dickens",
    "William Shakespeare",
    "Mark Twain",
    "Virginia Woolf",
    "F. Scott Fitzgerald",
    "Ernest Hemingway",
    "J.K. Rowling",
    "Stephen King",
    "Agatha Christie",
    "Isaac Asimov",
    "Philip K. Dick",
    "Ursula K. Le Guin",
    "Frank Herbert",
    "J.R.R. Tolkien",
    "George R.R. Martin",
];

const SAMPLE_TITLES: &[&str] = &[
    "Pride and Prejudice",
    "A Tale of Two Cities",
    "Romeo and Juliet",
    "The Adventures of Tom Sawyer",
    "Mrs. Dalloway",
    "The Great Gatsby",
    "The Old Man and the Sea",
    "Harry Potter and the Philosopher's Stone",
    "The Shining",
    "Murder on the Orient Express",
    "Foundation",
    "Do Androids Dream of Electric Sheep?",
    "The Left Hand of Darkness",
    "Dune",
    "The Lord of the Rings",
    "A Game of Thrones",
    "To Kill a Mockingbird",
    "1984",
    "The Catcher in the Rye",
    "One Hundred Years of Solitude",
    "Beloved",
    "The Handmaid's Tale",
    "Neuromancer",
    "The Hitchhiker's Guide to the Galaxy",
    "Ender's Game",
    "The Martian",
    "Gone Girl",
    "The Girl with the Dragon Tattoo",
    "The Kite Runner",
    "Life of Pi",
];

pub fn generate_sample_books(count: usize) -> Vec<BookCreateData> {
    let mut rng = rand::rng();
    let mut books = Vec::with_capacity(count);

    for i in 0..count {
        let author = SAMPLE_AUTHORS.choose(&mut rng).unwrap().to_string();

        // Create varied titles - some from templates, some completely random
        let title = if i % 3 == 0 {
            // Use sample titles
            SAMPLE_TITLES.choose(&mut rng).unwrap().to_string()
        } else {
            // Generate unique titles
            format!("Book {}", generate_unique_suffix())
        };

        books.push(BookCreateData { title, author });
    }

    books
}

fn generate_unique_suffix() -> String {
    use rand::Rng;
    let mut rng = rand::rng();

    // Generate a random string with letters and numbers
    let chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let len = rng.random_range(4..=12);

    (0..len)
        .map(|_| {
            let idx = rng.random_range(0..chars.len());
            chars.chars().nth(idx).unwrap()
        })
        .collect()
}

pub fn convert_to_create_in(books: Vec<BookCreateData>) -> Vec<BookCreateIn> {
    books
        .into_iter()
        .map(|book| BookCreateIn {
            primary_author_name: Some(book.author),
            work_title: book.title,
            primary_author_id: None,
            status: Some(BookCreateInStatus::Available),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_unique_suffix() {
        let suffix1 = generate_unique_suffix();
        let suffix2 = generate_unique_suffix();

        assert!(suffix1.len() >= 4);
        assert!(suffix1.len() <= 12);
        assert_ne!(suffix1, suffix2); // Very unlikely to be the same
    }

    #[test]
    fn test_convert_to_create_in() {
        let books = vec![BookCreateData {
            title: "Test Book".to_string(),
            author: "Test Author".to_string(),
        }];

        let create_ins = convert_to_create_in(books);
        assert_eq!(create_ins.len(), 1);
        assert_eq!(create_ins[0].work_title, "Test Book");
        assert_eq!(
            create_ins[0].primary_author_name,
            Some("Test Author".to_string())
        );
        assert_eq!(create_ins[0].primary_author_id, None);
        assert_eq!(create_ins[0].status, Some(BookCreateInStatus::Available));
    }
}
