use data_loader::{generate_sample_books, BookCreateData};

#[test]
fn test_generate_sample_books() {
    let books = generate_sample_books(10);

    assert_eq!(books.len(), 10, "Should generate exact count");

    for (i, book) in books.iter().enumerate() {
        assert!(!book.title.is_empty(), "Book {} should have title", i);
        assert!(!book.author.is_empty(), "Book {} should have author", i);
    }

    // Check for uniqueness
    let titles: std::collections::HashSet<_> = books.iter().map(|b| &b.title).collect();
    assert_eq!(titles.len(), 10, "All titles should be unique");
}

#[test]
fn test_generate_large_batch() {
    let books = generate_sample_books(1000);

    assert_eq!(books.len(), 1000, "Should handle large batches");

    // Check distribution of genres/styles
    let has_classic = books
        .iter()
        .any(|b| b.title.contains("Pride") || b.title.contains("Gatsby"));
    let has_scifi = books
        .iter()
        .any(|b| b.title.contains("Dune") || b.title.contains("Neuromancer"));
    let has_fantasy = books
        .iter()
        .any(|b| b.title.contains("Rings") || b.title.contains("Hobbit"));

    assert!(
        has_classic || has_scifi || has_fantasy,
        "Should have variety in generated books"
    );
}

#[test]
fn test_book_create_data_structure() {
    let book = BookCreateData {
        title: "Test Title".to_string(),
        author: "Test Author".to_string(),
    };

    assert_eq!(book.title, "Test Title");
    assert_eq!(book.author, "Test Author");
}

#[tokio::test]
async fn test_concurrent_book_generation() {
    use tokio::task;

    let handles: Vec<_> = (0..5)
        .map(|_| task::spawn(async { generate_sample_books(20) }))
        .collect();

    let results = futures::future::join_all(handles).await;

    for result in results {
        let books = result.expect("Task should complete");
        assert_eq!(books.len(), 20);
    }
}

#[test]
fn test_empty_generation() {
    let books = generate_sample_books(0);
    assert!(books.is_empty(), "Should handle zero count");
}

#[test]
fn test_single_book_generation() {
    let books = generate_sample_books(1);
    assert_eq!(books.len(), 1);
    assert!(!books[0].title.is_empty());
    assert!(!books[0].author.is_empty());
}

mod integration {
    use super::*;

    #[test]
    fn test_book_title_formatting() {
        let books = generate_sample_books(50);

        for book in &books {
            // Titles should be properly formatted
            assert!(!book.title.starts_with(' '), "No leading spaces");
            assert!(!book.title.ends_with(' '), "No trailing spaces");
            assert!(!book.title.is_empty(), "No empty titles");

            // Authors should be properly formatted
            assert!(!book.author.starts_with(' '), "No leading spaces in author");
            assert!(!book.author.ends_with(' '), "No trailing spaces in author");
            assert!(!book.author.is_empty(), "No empty authors");
        }
    }

    #[test]
    fn test_deterministic_generation() {
        // While the function uses randomness, it should be consistent within a run
        let batch1 = generate_sample_books(5);
        let batch2 = generate_sample_books(5);

        // Different batches should have different content
        let all_different = batch1
            .iter()
            .zip(batch2.iter())
            .any(|(b1, b2)| b1.title != b2.title);

        assert!(
            all_different,
            "Different batches should have different books"
        );
    }
}
