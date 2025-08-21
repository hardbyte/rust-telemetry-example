#!/usr/bin/env python3
"""
Open Library Loader: Subject Import with UPSERTs and Outbox Event Appends

This script loads works (and optionally editions) from Open Library by subject and
upserts them into the normalized schema:

- authors (unique by name)
- works (unique by (title, publication_year))
- work_authors (primary author association)
- editions (unique by isbn) [optional]

It also appends outbox events to the `events` table for reliable publishing to Kafka:
- work_created (always when --emit-events)
- edition_added (optional per edition when --emit-edition-events)

Requirements:
- Python 3.10+
- requests
- psycopg[binary] (psycopg 3)

Quick start (without polluting your environment):
- Ensure Postgres is running and schema is migrated
  export DATABASE_URL="postgres://postgres:password@localhost:5432/bookapp"
  cd bookapp-dal && sqlx migrate run

- Run via uv:
  uvx --with requests --with "psycopg[binary]" python bookapp-dal/loaders/openlibrary_loader.py \
    --subject "science_fiction" --limit 100 --editions-per-work 0 --emit-events

Notes:
- Idempotent via UPSERTs.
- Batches are processed in DB transactions for safety.
- Events include `topic` to integrate with the outbox publisher (default: domain.events).
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import logging
import os
import sys
import time
from typing import Any, Dict, Iterable, Optional, Tuple

import requests
import psycopg
from psycopg.rows import dict_row
from psycopg.types.json import Json


OL_BASE = "https://openlibrary.org"
DEFAULT_PAGE_SIZE = 50
DEFAULT_BATCH_SIZE = 50
DEFAULT_LIMIT = 100
DEFAULT_TOPIC = os.environ.get("OUTBOX_DEFAULT_TOPIC", "domain.events")

logger = logging.getLogger("openlibrary_loader")


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description="Open Library subject loader with UPSERTs and outbox events")
    p.add_argument("--subject", required=True, help="Open Library subject (e.g., 'science_fiction')")
    p.add_argument("--limit", type=int, default=DEFAULT_LIMIT, help="Total works to import (default: %(default)s)")
    p.add_argument("--page-size", type=int, default=DEFAULT_PAGE_SIZE, help="Page size for Open Library API calls")
    p.add_argument("--batch-size", type=int, default=DEFAULT_BATCH_SIZE, help="DB transaction batch size")
    p.add_argument("--editions-per-work", type=int, default=0, help="Fetch up to N editions per work (0=skip)")
    p.add_argument("--emit-events", action="store_true", default=True, help="Append outbox events (default: true)")
    p.add_argument("--no-events", dest="emit_events", action="store_false", help="Disable event appends")
    p.add_argument("--emit-edition-events", action="store_true", default=False, help="Emit edition_added events")
    p.add_argument("--topic", default=DEFAULT_TOPIC, help=f"Outbox topic (default: {DEFAULT_TOPIC})")
    p.add_argument("--source-service", default="loader", help="events.source_service value")
    p.add_argument("--dry-run", action="store_true", help="Do not write to DB")
    p.add_argument("--verbose", "-v", action="count", default=0, help="Increase logging verbosity")
    return p.parse_args()


def setup_logging(verbosity: int) -> None:
    level = logging.WARNING
    if verbosity == 1:
        level = logging.INFO
    elif verbosity >= 2:
        level = logging.DEBUG
    logging.basicConfig(level=level, format="%(asctime)s %(levelname)s %(name)s - %(message)s")


def get_db_conn() -> psycopg.Connection:
    db_url = os.environ.get("DATABASE_URL")
    if not db_url:
        logger.error("DATABASE_URL is not set")
        sys.exit(2)
    # Use autocommit False; we'll explicitly manage transactions
    return psycopg.connect(db_url, autocommit=False, row_factory=dict_row)


def http_get_json(url: str, params: Optional[Dict[str, Any]] = None, retries: int = 3, backoff: float = 0.8) -> Dict[str, Any]:
    last_err = None
    for attempt in range(1, retries + 1):
        try:
            resp = requests.get(url, params=params, timeout=20)
            if resp.status_code == 200:
                return resp.json()
            else:
                logger.warning("GET %s -> %s; body=%s", resp.url, resp.status_code, resp.text[:200])
        except Exception as e:
            last_err = e
            logger.warning("GET %s failed (attempt %d/%d): %s", url, attempt, retries, e)
        time.sleep(backoff * attempt)
    if last_err:
        raise last_err
    raise RuntimeError(f"Failed to GET {url}")


def pick_primary_author_name(work_item: Dict[str, Any]) -> Optional[str]:
    # Open Library subject works have "authors": [{"name": "...", "key": "/authors/..."}, ...]
    authors = work_item.get("authors") or []
    if not authors:
        return None
    a0 = authors[0]
    name = a0.get("name")
    return name


def language_key_to_code(lang_key: str) -> Optional[str]:
    # e.g., "/languages/eng" -> "en"
    if not lang_key:
        return None
    code = lang_key.split("/")[-1]
    # crude mapping for common ones
    mapping = {
        "eng": "en",
        "spa": "es",
        "fre": "fr",
        "fra": "fr",
        "ger": "de",
        "deu": "de",
        "ita": "it",
        "por": "pt",
        "rus": "ru",
        "jpn": "ja",
        "zho": "zh",
        "chi": "zh",
    }
    return mapping.get(code, code[:2] if code else None)


def parse_publish_date(s: Optional[str]) -> Optional[dt.date]:
    if not s:
        return None
    # Try a few common formats; fall back to None
    for fmt in ("%Y-%m-%d", "%Y-%m", "%Y"):
        try:
            return dt.datetime.strptime(s, fmt).date()
        except Exception:
            pass
    # Very loose fallbacks: try splitting on space and last token as year
    parts = s.strip().split()
    if parts and parts[-1].isdigit() and len(parts[-1]) == 4:
        try:
            return dt.date(int(parts[-1]), 1, 1)
        except Exception:
            return None
    return None


def upsert_author(cur: psycopg.Cursor, name: str, sort_name: Optional[str] = None) -> int:
    sql = """
    INSERT INTO authors (name, sort_name)
    VALUES (%s, %s)
    ON CONFLICT (name) DO UPDATE
    SET sort_name = COALESCE(EXCLUDED.sort_name, authors.sort_name)
    RETURNING id
    """
    cur.execute(sql, (name, sort_name))
    author_id = cur.fetchone()["id"]
    logger.debug("author upserted: %s -> id=%s", name, author_id)
    return author_id


def upsert_work(
    cur: psycopg.Cursor,
    title: str,
    publication_year: Optional[int],
    description: Optional[str],
    original_language: Optional[str],
) -> int:
    sql = """
    INSERT INTO works (title, original_language, description, publication_year)
    VALUES (%s, %s, %s, %s)
    ON CONFLICT (title, publication_year) DO UPDATE
    SET description = COALESCE(EXCLUDED.description, works.description),
        original_language = COALESCE(EXCLUDED.original_language, works.original_language)
    RETURNING id
    """
    cur.execute(sql, (title, original_language, description, publication_year))
    work_id = cur.fetchone()["id"]
    logger.debug("work upserted: %s (%s) -> id=%s", title, publication_year, work_id)
    return work_id


def upsert_work_primary_author(cur: psycopg.Cursor, work_id: int, author_id: int) -> None:
    sql = """
    INSERT INTO work_authors (work_id, author_id, role, primary_author, ord)
    VALUES (%s, %s, 'Author', TRUE, 1)
    ON CONFLICT (work_id, author_id) DO UPDATE
    SET primary_author = TRUE,
        ord = 1,
        role = EXCLUDED.role
    """
    cur.execute(sql, (work_id, author_id))


def upsert_edition(
    cur: psycopg.Cursor,
    work_id: int,
    isbn: str,
    title: Optional[str],
    publisher: Optional[str],
    publication_date: Optional[dt.date],
    language: Optional[str],
    page_count: Optional[int],
    fmt: Optional[str],
) -> Optional[int]:
    sql = """
    INSERT INTO editions (work_id, isbn, title, publisher, publication_date, "language", page_count, format)
    VALUES (%s, %s, %s, %s, %s, %s, %s, %s)
    ON CONFLICT (isbn) DO NOTHING
    RETURNING id
    """
    cur.execute(sql, (work_id, isbn, title, publisher, publication_date, language, page_count, fmt))
    row = cur.fetchone()
    if row:
        ed_id = row["id"]
        logger.debug("edition inserted: isbn=%s -> id=%s", isbn, ed_id)
        return ed_id
    # Already existed; fetch id for reference
    cur.execute("SELECT id FROM editions WHERE isbn = %s", (isbn,))
    row = cur.fetchone()
    ed_id = row["id"] if row else None
    logger.debug("edition upserted (existing): isbn=%s -> id=%s", isbn, ed_id)
    return ed_id


def append_event(
    cur: psycopg.Cursor,
    aggregate_type: str,
    aggregate_id: str,
    event_type: str,
    payload: Dict[str, Any],
    source_service: str,
    topic: str,
    version: int = 1,
) -> int:
    sql = """
    INSERT INTO events
      (aggregate_type, aggregate_id, event_type, payload, headers, trace_id, span_id, source_service, version, topic)
    VALUES
      (%s,             %s,            %s,         %s,      %s,      %s,       %s,      %s,             %s,      %s)
    RETURNING id
    """
    cur.execute(
        sql,
        (
            aggregate_type,
            aggregate_id,
            event_type,
            Json(payload),
            Json({}),
            None,
            None,
            source_service,
            version,
            topic,
        ),
    )
    event_id = cur.fetchone()["id"]
    logger.debug("event appended: id=%s type=%s agg=%s:%s", event_id, event_type, aggregate_type, aggregate_id)
    return event_id


def fetch_subject_page(subject: str, limit: int, offset: int) -> Dict[str, Any]:
    url = f"{OL_BASE}/subjects/{subject}.json"
    params = {"limit": limit, "offset": offset}
    return http_get_json(url, params=params)


def fetch_editions_for_work(work_key: str, limit: int) -> Iterable[Dict[str, Any]]:
    # work_key like "/works/OL12345W"
    url = f"{OL_BASE}{work_key}/editions.json"
    params = {"limit": limit}
    data = http_get_json(url, params=params)
    editions = data.get("entries") or data.get("editions") or []
    return editions


def pick_isbn(entry: Dict[str, Any]) -> Optional[str]:
    # Prefer ISBN-13
    isbn13 = entry.get("isbn_13") or []
    if isinstance(isbn13, list) and isbn13:
        return isbn13[0]
    isbn10 = entry.get("isbn_10") or []
    if isinstance(isbn10, list) and isbn10:
        return isbn10[0]
    # Some records store identifiers under "identifiers": {"isbn_13": [...]} etc.
    identifiers = entry.get("identifiers") or {}
    i13 = identifiers.get("isbn_13") or []
    if isinstance(i13, list) and i13:
        return i13[0]
    i10 = identifiers.get("isbn_10") or []
    if isinstance(i10, list) and i10:
        return i10[0]
    return None


def process_work_item(
    cur: psycopg.Cursor,
    item: Dict[str, Any],
    fetch_editions: int,
    emit_events: bool,
    emit_edition_events: bool,
    topic: str,
    source_service: str,
) -> Tuple[Optional[int], Optional[str]]:
    """
    Upsert author/work/association, optionally editions, and append events.
    Returns (work_id, primary_author_name) or (None, None) if skipped.
    """
    title = (item.get("title") or "").strip()
    if not title:
        logger.info("Skipping item with no title")
        return None, None

    author_name = pick_primary_author_name(item)
    if not author_name:
        logger.info("Skipping work '%s' with no author", title)
        return None, None

    publication_year = item.get("first_publish_year")
    description = None
    # Subject API doesn't include rich description; leave None
    original_language = None

    author_id = upsert_author(cur, author_name, None)
    work_id = upsert_work(cur, title, publication_year, description, original_language)
    upsert_work_primary_author(cur, work_id, author_id)

    if emit_events:
        payload = {
            "work_id": work_id,
            "title": title,
            "primary_author_name": author_name,
            "publication_year": publication_year,
        }
        append_event(cur, "work", str(work_id), "work_created", payload, source_service, topic, version=1)

    if fetch_editions > 0:
        work_key = item.get("key")  # e.g. "/works/OL27448W"
        if isinstance(work_key, str) and work_key.startswith("/works/"):
            try:
                editions = list(fetch_editions_for_work(work_key, fetch_editions))
            except Exception as e:
                logger.warning("Failed to fetch editions for %s: %s", work_key, e)
                editions = []
            for ed in editions:
                isbn = pick_isbn(ed)
                if not isbn:
                    continue
                ed_title = ed.get("title")
                publishers = ed.get("publishers") or []
                publisher = publishers[0] if publishers else None
                pub_date = parse_publish_date(ed.get("publish_date"))
                languages = ed.get("languages") or []
                lang = language_key_to_code(languages[0].get("key")) if languages and isinstance(languages[0], dict) else None
                page_count = ed.get("number_of_pages")
                fmt = ed.get("physical_format")

                ed_id = upsert_edition(cur, work_id, isbn, ed_title, publisher, pub_date, lang, page_count, fmt)

                if emit_events and emit_edition_events and ed_id:
                    payload = {
                        "edition_id": ed_id,
                        "work_id": work_id,
                        "isbn": isbn,
                        "title": ed_title,
                        "publisher": publisher,
                        "publication_date": pub_date.isoformat() if pub_date else None,
                        "language": lang,
                        "page_count": page_count,
                        "format": fmt,
                    }
                    append_event(cur, "edition", f"isbn:{isbn}", "edition_added", payload, source_service, topic, version=1)

    return work_id, author_name


def main() -> None:
    args = parse_args()
    setup_logging(args.verbose)

    logger.info(
        "Starting loader subject=%s limit=%d page_size=%d batch_size=%d editions_per_work=%d emit_events=%s topic=%s dry_run=%s",
        args.subject,
        args.limit,
        args.page_size,
        args.batch_size,
        args.editions_per_work,
        args.emit_events,
        args.topic,
        args.dry_run,
    )

    # Fetch-and-load loop
    processed = 0
    offset = 0

    conn: Optional[psycopg.Connection] = None
    if not args.dry_run:
        conn = get_db_conn()

    try:
        while processed < args.limit:
            page_limit = min(args.page_size, args.limit - processed)
            data = fetch_subject_page(args.subject, page_limit, offset)
            works = data.get("works") or []
            if not works:
                logger.info("No more works from API (processed=%d)", processed)
                break

            logger.info("Fetched %d works from subject '%s' (offset=%d)", len(works), args.subject, offset)

            # Process batch in a single transaction
            if not args.dry_run:
                cur = conn.cursor()
            else:
                cur = None  # type: ignore[assignment]

            batch_count = 0
            try:
                for item in works:
                    if processed >= args.limit:
                        break
                    if batch_count >= args.batch_size and not args.dry_run:
                        # Commit current transaction and start a new one for the remaining items
                        conn.commit()
                        cur.close()
                        cur = conn.cursor()
                        batch_count = 0

                    if args.dry_run:
                        # Simulate processing without DB writes
                        title = (item.get("title") or "").strip()
                        author_name = pick_primary_author_name(item)
                        logger.info("[DRY RUN] Would process: title=%r author=%r", title, author_name)
                        processed += 1
                        batch_count += 1
                        continue

                    # Process with DB
                    process_work_item(
                        cur,
                        item,
                        fetch_editions=args.editions_per_work,
                        emit_events=args.emit_events,
                        emit_edition_events=args.emit_edition_events,
                        topic=args.topic,
                        source_service=args.source_service,
                    )
                    processed += 1
                    batch_count += 1

                if not args.dry_run:
                    conn.commit()
                    cur.close()

            except Exception as e:
                logger.error("Batch failed: %s", e, exc_info=True)
                if not args.dry_run:
                    conn.rollback()
                    try:
                        cur.close()
                    except Exception:
                        pass
                # Continue to next page
            finally:
                pass

            offset += len(works)
            # Play nice with rate limits
            time.sleep(0.2)

        logger.info("Completed load: processed=%d subject=%s", processed, args.subject)

    finally:
        if conn is not None:
            try:
                conn.close()
            except Exception:
                pass


if __name__ == "__main__":
    main()
