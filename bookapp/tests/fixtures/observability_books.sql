-- Observability Test Fixtures

-- 1. Metrics Test Data
INSERT INTO works (id, title) VALUES ('01934e01-0000-7000-8000-000000000001', 'Metrics Test');
INSERT INTO authors (id, name) VALUES ('01934e02-0000-7000-8000-000000000001', 'Metrics Author');
INSERT INTO work_authors (work_id, author_id, primary_author) VALUES ('01934e01-0000-7000-8000-000000000001', '01934e02-0000-7000-8000-000000000001', true);
INSERT INTO book_search_index (work_id, work_title, primary_author_name, author_names, document) 
VALUES ('01934e01-0000-7000-8000-000000000001', 'Metrics Test', 'Metrics Author', 'Metrics Author', 
        setweight(to_tsvector('english', 'Metrics Test'), 'A') || setweight(to_tsvector('english', 'Metrics Author'), 'B'));

-- 2. Tracing Test Data
INSERT INTO works (id, title) VALUES ('01934e01-0000-7000-8000-000000000002', 'Tracing Test');
INSERT INTO authors (id, name) VALUES ('01934e02-0000-7000-8000-000000000002', 'Tracing Author');
INSERT INTO work_authors (work_id, author_id, primary_author) VALUES ('01934e01-0000-7000-8000-000000000002', '01934e02-0000-7000-8000-000000000002', true);
INSERT INTO book_search_index (work_id, work_title, primary_author_name, author_names, document) 
VALUES ('01934e01-0000-7000-8000-000000000002', 'Tracing Test', 'Tracing Author', 'Tracing Author', 
        setweight(to_tsvector('english', 'Tracing Test'), 'A') || setweight(to_tsvector('english', 'Tracing Author'), 'B'));

-- 3. Logging Test Data
INSERT INTO works (id, title) VALUES ('01934e01-0000-7000-8000-000000000003', 'Logging Test');
INSERT INTO authors (id, name) VALUES ('01934e02-0000-7000-8000-000000000003', 'Logging Author');
INSERT INTO work_authors (work_id, author_id, primary_author) VALUES ('01934e01-0000-7000-8000-000000000003', '01934e02-0000-7000-8000-000000000003', true);
INSERT INTO book_search_index (work_id, work_title, primary_author_name, author_names, document) 
VALUES ('01934e01-0000-7000-8000-000000000003', 'Logging Test', 'Logging Author', 'Logging Author', 
        setweight(to_tsvector('english', 'Logging Test'), 'A') || setweight(to_tsvector('english', 'Logging Author'), 'B'));

-- 4. Correlation Test Data
INSERT INTO works (id, title) VALUES ('01934e01-0000-7000-8000-000000000004', 'Correlation E2E');
INSERT INTO authors (id, name) VALUES ('01934e02-0000-7000-8000-000000000004', 'E2E Author');
INSERT INTO work_authors (work_id, author_id, primary_author) VALUES ('01934e01-0000-7000-8000-000000000004', '01934e02-0000-7000-8000-000000000004', true);
INSERT INTO book_search_index (work_id, work_title, primary_author_name, author_names, document) 
VALUES ('01934e01-0000-7000-8000-000000000004', 'Correlation E2E', 'E2E Author', 'E2E Author', 
        setweight(to_tsvector('english', 'Correlation E2E'), 'A') || setweight(to_tsvector('english', 'E2E Author'), 'B'));
