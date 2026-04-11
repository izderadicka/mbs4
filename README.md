# mbs4

mbs4 is a Rust application for managing an ebook library.

mbs4 = My BookShelf 4

It's successor of [mybookshelf2](https://github.com/izderadicka/mybookshelf2). 

Historically I wanted application that can have multiple files of same type per ebook - which was limitation of Calibre - and with simple web based access so that was functional motivation.

But key motivation was to to play with technologies (mbs1 was python+Django, mbs2 was python+flask+asyncio+postgres, mbs3 was Java+Quarkus (microservice architecture, just PoC) and mbs4 is Rust), so some parts of solution might be bit overkill, others underdeveloped.

Why Rust -  maybe it's not best language for this type of application, but I do like Rust - and wanted to improve on idiomatic code and try Axum - so it's about using Rust in known "domain", which I have tried already in different stacks.

Web client (Typescript/Svelte5 SPA) [mbs4-client](https://github.com/izderadicka/mbs4-client/) - nothing special as I'm not so much on front-end side.

## Features

- local (username+password) or OIDC authentication
- user's cookie or bearer-token based API access
- very simple authorization based on roles (normal user (default), trusted user(Trusted), admin(Admin))
- ebook records with authors, genres, languages, series, and files
- users' bookshelf collections
- file uploads and downloads
- metadata extraction from uploaded ebook files
- ebook format conversion
- search
- server-sent events for background operations
- optional OpenAPI / Swagger UI


## Tech stack

- Rust workspace
- Axum for the HTTP server
- SQLx with SQLite
- Tokio async runtime
- Utoipa + Swagger UI for API docs
- Calibre command-line tools for ebook metadata extraction and conversion
- LibreOffice-based preprocessing for some document conversions

## Run with Docker

The easiest way to run mbs4 is with Docker.

Start from [docker-compose-template.yml](/home/ivan/workspace/mbs4/docker-compose-template.yml:1), adjust the paths and host name for your environment, and run it with Docker Compose.

The compose template is set up to:

- store persistent data in a mounted `/data` directory
- optionally serve a built frontend from `/client`
- expose the application on port `4000`

After startup, open:

```text
http://localhost:4000/health
http://localhost:4000/swagger-ui
```

## CLI

Right now, the CLI is the only way to create users.

Create the first admin user with:

```bash
cargo run -p mbs4-cli -- create-user \
  --data-dir ./data \
  --name "Admin" \
  --email admin@localhost \
  --password password
```

You can use `--help` on the server and CLI commands for the rest.

## Run locally

If you do not want to use Docker, you can run the server directly with Cargo.

For metadata extraction and ebook conversion, mbs4 expects Calibre command-line tools to be installed. Some document conversions (.doc) also rely on LibreOffice.

## Workspace layout

The project is split into focused crates:

- `mbs4-server`: Axum server binary and runtime wiring
- `mbs4-app`: application logic and HTTP route handlers
- `mbs4-dal`: database access layer
- `mbs4-types`: shared types and config structures
- `mbs4-auth`: token and OIDC authentication support
- `mbs4-search`: search/indexing logic
- `mbs4-store`: file storage and download handling
- `mbs4-image`: image processing helpers
- `mbs4-calibre`: integration with Calibre and document conversion tools
- `mbs4-cli`: command-line utility for admin/import tasks
- `mbs4-e2e-tests`: end-to-end integration tests
- `mbs4-macros`: internal proc macros for DAL - repository pattern

## License

MIT or Apache-2.0 - you choose
