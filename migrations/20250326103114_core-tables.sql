
        CREATE TABLE language (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            version INTEGER NOT NULL,
            code       TEXT NOT NULL UNIQUE,
            name       TEXT NOT NULL
        );
        

        CREATE TABLE series (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            version INTEGER NOT NULL,
            created TEXT NOT NULL,
            -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
            modified TEXT NOT NULL,
            -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
            title TEXT NOT NULL,
            rating REAL,
            rating_count INTEGER,
            description TEXT,
            created_by TEXT
        );
        -- Creating indexes
        CREATE INDEX ix_series_modified ON series(modified);
        CREATE INDEX ix_series_title ON series(title);
        CREATE INDEX ix_series_rating_desc ON series (rating DESC);
    

        CREATE TABLE author (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            version        INTEGER NOT NULL,
            created        TEXT NOT NULL,  -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
            modified       TEXT NOT NULL,  -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
            last_name      TEXT NOT NULL,
            first_name     TEXT,
            description    TEXT,
            created_by  TEXT
        );

        -- Creating indexes
        CREATE INDEX ix_author_modified ON author(modified);
        CREATE INDEX ix_author_name ON author(last_name, first_name);

        

        CREATE TABLE ebook (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            version     INTEGER NOT NULL,
            created        TEXT NOT NULL,  -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
            modified       TEXT NOT NULL,  -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
            title          TEXT NOT NULL,
            description    TEXT,
            language_id    INTEGER NOT NULL,
            series_id      INTEGER,
            series_index   INTEGER,
            rating         REAL,  -- Equivalent to double precision
            rating_count   INTEGER,
            --downloads      INTEGER, -- was not used in old version
            cover          TEXT,
            base_dir       TEXT NOT NULL,
            created_by  TEXT,
            FOREIGN KEY (language_id) REFERENCES language(id),
            FOREIGN KEY (series_id) REFERENCES series(id)
        );

        -- Creating indexes
        CREATE INDEX ix_ebook_modified ON ebook(modified);
        CREATE INDEX ix_ebook_series_id ON ebook(series_id);
        CREATE INDEX ix_ebook_language_id ON ebook(language_id);
        CREATE INDEX ix_ebook_title ON ebook(title);
        CREATE INDEX ix_ebook_rating_desc ON ebook (rating DESC);

        

        CREATE TABLE genre (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        version INTEGER NOT NULL,
        name       TEXT NOT NULL UNIQUE
        );

        

        CREATE TABLE format (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            version INTEGER NOT NULL,
            mime_type  TEXT NOT NULL,
            name       TEXT NOT NULL,
            extension  TEXT NOT NULL UNIQUE
        );

        

        CREATE TABLE source (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            version     INTEGER NOT NULL,
            created        TEXT NOT NULL,  -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
            modified       TEXT NOT NULL,  -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
            ebook_id       INTEGER NOT NULL,
            location       TEXT NOT NULL,
            --load_source    TEXT,
            format_id      INTEGER NOT NULL,
            size           INTEGER NOT NULL,
            hash           TEXT NOT NULL,
            quality        REAL,  -- Equivalent to double precision
            --quality_count  INTEGER,
            created_by     TEXT,
            FOREIGN KEY (ebook_id) REFERENCES ebook(id) ON DELETE CASCADE,
            FOREIGN KEY (format_id) REFERENCES format(id)
        );

        -- Creating indexes
        CREATE INDEX ix_source_modified ON source(modified);
        CREATE INDEX ix_source_ebook_id ON source(ebook_id);
        CREATE INDEX ix_source_format_id ON source(format_id);
        CREATE INDEX ix_source_hash ON source(hash);

        

        CREATE TABLE bookshelf (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            version     INTEGER NOT NULL,
            created        TEXT NOT NULL,  -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
            modified       TEXT NOT NULL,  -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
            name          TEXT NOT NULL,
            description    TEXT,
            public         INTEGER,
            rating         REAL,  -- Equivalent to double precision
            rating_count   INTEGER,
            created_by  TEXT
            );

            CREATE INDEX ix_bookshelf_modified ON bookshelf (modified);
            CREATE INDEX ix_bookshelf_name     ON bookshelf (name);
            CREATE INDEX ix_bookshelf_rating_desc ON bookshelf (rating DESC);
            CREATE INDEX ix_created_by ON bookshelf (created_by);
        

        CREATE TABLE bookshelf_item (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            version        INTEGER NOT NULL,
            created        TEXT NOT NULL,  -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
            modified       TEXT NOT NULL,  -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
            type            TEXT NOT NULL,         -- BOOKSHELF_ITEM_TYPE enum -> TEXT
            bookshelf_id    INTEGER NOT NULL,
            ebook_id        INTEGER,
            series_id       INTEGER,
            "order"         INTEGER,               -- reserved word, so quoted
            note            TEXT,
            created_by  TEXT,
            FOREIGN KEY (bookshelf_id)   REFERENCES bookshelf(id) ON DELETE CASCADE,
            FOREIGN KEY (ebook_id)       REFERENCES ebook(id),
            FOREIGN KEY (series_id)      REFERENCES series(id),
            UNIQUE (bookshelf_id, ebook_id),
            UNIQUE (bookshelf_id, series_id)
        );

        CREATE INDEX ix_bookshelf_item_modified ON bookshelf_item (modified);
        CREATE INDEX ix_bookshelf_item_bookshelf_id ON bookshelf_item (bookshelf_id);

        

        CREATE TABLE conversion_batch (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            created        TEXT NOT NULL,  -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
            name            TEXT NOT NULL,         -- varchar(100) -> TEXT
            for_entity      TEXT,                  -- enum CONVERSION_BATCH_ENTITY -> TEXT
            entity_id       INTEGER,
            format_id       INTEGER NOT NULL,
            zip_location    TEXT,          
            created_by  TEXT,
           
            FOREIGN KEY (format_id)       REFERENCES format(id)

        );

        -- Creating indexes

        CREATE INDEX ix_conversion_batch_created ON conversion_batch (created);
       
        

        CREATE TABLE conversion (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            created        TEXT NOT NULL,  -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
            batch_id        INTEGER,
            source_id       INTEGER NOT NULL,
            location        TEXT NOT NULL,         -- varchar(512) -> TEXT
            format_id       INTEGER NOT NULL,
            created_by  TEXT,
            FOREIGN KEY (batch_id)        REFERENCES conversion_batch(id),
            FOREIGN KEY (source_id)       REFERENCES source(id) ON DELETE CASCADE,
            FOREIGN KEY (format_id)       REFERENCES format(id)

        );

        -- Creating indexes
        CREATE INDEX ix_conversion_source_id ON conversion(source_id);
        CREATE INDEX ix_conversion_format_id ON conversion(format_id);
        CREATE INDEX ix_conversion_created ON conversion (created);
        

        CREATE TABLE ebook_rating (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            version     INTEGER NOT NULL,
            created        TEXT NOT NULL,  -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
            modified       TEXT NOT NULL,  -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
            ebook_id       INTEGER NOT NULL,
            rating          REAL,                  -- double precision
            description     TEXT,
            created_by  TEXT
        );

        -- Creating indexes
        CREATE INDEX ix_ebook_rating_modified ON ebook_rating(modified);
        CREATE INDEX ix_ebook_rating_ebook_id ON ebook_rating(ebook_id);
        CREATE INDEX ix_ebook_rating_created_by ON ebook_rating(created_by);
        

        

        CREATE TABLE series_rating (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            version     INTEGER NOT NULL,
            created        TEXT NOT NULL,  -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
            modified       TEXT NOT NULL,  -- Stored as ISO 8601 string (YYYY-MM-DD HH:MM:SS)
            series_id       INTEGER NOT NULL,
            rating          REAL,                  -- double precision
            description     TEXT,
            created_by  TEXT
        );

        -- Creating indexes
        CREATE INDEX ix_series_rating_modified ON series_rating(modified);
        CREATE INDEX ix_series_rating_series_id ON series_rating(series_id);
        CREATE INDEX ix_series_rating_created_by ON series_rating(created_by);
        

        


            CREATE TABLE  ebook_authors (
                ebook_id INTEGER NOT NULL,
                author_id INTEGER NOT NULL,
                PRIMARY KEY (ebook_id, author_id),
                FOREIGN KEY (ebook_id) REFERENCES ebook (id) ON DELETE CASCADE,
                FOREIGN KEY (author_id) REFERENCES author (id) ON DELETE CASCADE
            );

            -- Creating indexes for optimized lookups
            CREATE INDEX ix_ebook_authors_author_id ON ebook_authors(author_id);
            CREATE INDEX ix_ebook_authors_ebook_id ON ebook_authors(ebook_id);
        


            CREATE TABLE  ebook_genres (
                ebook_id INTEGER NOT NULL,
                genre_id INTEGER NOT NULL,
                PRIMARY KEY (ebook_id, genre_id),
                FOREIGN KEY (ebook_id) REFERENCES ebook (id) ON DELETE CASCADE,
                FOREIGN KEY (genre_id) REFERENCES genre (id) ON DELETE CASCADE
            );

            -- Creating indexes for optimized lookups
            CREATE INDEX ix_ebook_genres_genre_id ON ebook_genres(genre_id);
            CREATE INDEX ix_ebook_genres_ebook_id ON ebook_genres(ebook_id);
        
