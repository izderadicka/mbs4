-- Add migration script here
CREATE TABLE language (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    version INTEGER NOT NULL,
    code TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL
);
CREATE TABLE series (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    version INTEGER NOT NULL,
    created TEXT NOT NULL,
    -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
    modified TEXT NOT NULL,
    -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
    title TEXT NOT NULL,
    --rating REAL,
    --rating_count INTEGER,
    description TEXT,
    created_by TEXT
);
-- Creating indexes
CREATE INDEX ix_series_modified ON series(modified);
CREATE INDEX ix_series_title ON series(title);
CREATE TABLE author (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    version INTEGER NOT NULL,
    created TEXT NOT NULL,
    -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
    modified TEXT NOT NULL,
    -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
    last_name TEXT NOT NULL,
    first_name TEXT,
    description TEXT,
    created_by TEXT
);
-- Creating indexes
CREATE INDEX ix_author_modified ON author(modified);
CREATE INDEX ix_author_name ON author(last_name, first_name);
CREATE TABLE ebook (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    version INTEGER NOT NULL,
    created TEXT NOT NULL,
    -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
    modified TEXT NOT NULL,
    -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
    title TEXT NOT NULL,
    description TEXT,
    language_id INTEGER NOT NULL,
    series_id INTEGER,
    series_index INTEGER,
    --rating         REAL,  -- Equivalent to double precision
    --rating_count   INTEGER,
    --downloads      INTEGER,
    cover TEXT,
    base_dir TEXT NOT NULL,
    created_by TEXT,
    FOREIGN KEY (language_id) REFERENCES language(id),
    FOREIGN KEY (series_id) REFERENCES series(id)
);
-- Creating indexes
CREATE INDEX ix_ebook_modified ON ebook(modified);
CREATE INDEX ix_ebook_series_id ON ebook(series_id);
CREATE INDEX ix_ebook_language_id ON ebook(language_id);
CREATE INDEX ix_ebook_title ON ebook(title);
CREATE TABLE genre (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    version INTEGER NOT NULL,
    name TEXT NOT NULL UNIQUE
);
CREATE TABLE format (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    version INTEGER NOT NULL,
    mime_type TEXT NOT NULL,
    name TEXT NOT NULL,
    extension TEXT NOT NULL UNIQUE
);
CREATE TABLE source (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    version INTEGER NOT NULL,
    created TEXT NOT NULL,
    -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
    modified TEXT NOT NULL,
    -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
    ebook_id INTEGER NOT NULL,
    location TEXT NOT NULL,
    --load_source    TEXT,
    format_id INTEGER NOT NULL,
    size INTEGER NOT NULL,
    hash TEXT NOT NULL,
    quality REAL,
    -- Equivalent to double precision
    --quality_count  INTEGER,
    created_by TEXT,
    FOREIGN KEY (ebook_id) REFERENCES ebook(id) ON DELETE CASCADE,
    FOREIGN KEY (format_id) REFERENCES format(id)
);
-- Creating indexes
CREATE INDEX ix_source_modified ON source(modified);
CREATE TABLE ebook_authors (
    ebook_id INTEGER NOT NULL,
    author_id INTEGER NOT NULL,
    PRIMARY KEY (ebook_id, author_id),
    FOREIGN KEY (ebook_id) REFERENCES ebook (id) ON DELETE CASCADE,
    FOREIGN KEY (author_id) REFERENCES author (id) ON DELETE CASCADE
);
-- Creating indexes for optimized lookups
CREATE INDEX ix_ebook_authors_author_id ON ebook_authors(author_id);
CREATE INDEX ix_ebook_authors_ebook_id ON ebook_authors(ebook_id);
CREATE TABLE ebook_genres (
    ebook_id INTEGER NOT NULL,
    genre_id INTEGER NOT NULL,
    PRIMARY KEY (ebook_id, genre_id),
    FOREIGN KEY (ebook_id) REFERENCES ebook (id) ON DELETE CASCADE,
    FOREIGN KEY (genre_id) REFERENCES genre (id) ON DELETE CASCADE
);
-- Creating indexes for optimized lookups
CREATE INDEX ix_ebook_genres_genre_id ON ebook_genres(genre_id);
CREATE INDEX ix_ebook_genres_ebook_id ON ebook_genres(ebook_id);