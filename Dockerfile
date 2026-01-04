FROM ubuntu:24.04 AS build
ARG build_type=debug
ENV DEBIAN_FRONTEND=noninteractive
ENV CARGO_HOME=/usr/local/cargo
ENV RUSTUP_HOME=/usr/local/rustup
ENV PATH=${CARGO_HOME}/bin:${PATH}

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl build-essential pkg-config libssl-dev git \
 && rm -rf /var/lib/apt/lists/*

# Install rustup + stable toolchain (non-interactive)
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain stable

# Install sqlx-cli
RUN cargo install sqlx-cli

WORKDIR /src
COPY . .

# CREATE DATABASE
RUN mkdir -p /src/test-data
ENV DATABASE_URL=sqlite:///src/test-data/mbs4.db
RUN sqlx database create && sqlx migrate run
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    set -eu; \
    mkdir -p /out; \
    case "${build_type}" in \
      debug) \
        cargo build; \
        cp target/debug/mbs4-server /out/mbs4-server ; \
        cp target/debug/mbs4-cli /out/mbs4-cli ;; \
      release) \
        cargo build --release; \
        cp target/release/mbs4-server /out/mbs4-server ; \
        cp target/release/mbs4-cli /out/mbs4-cli ;; \
      *) \
        echo "ERROR: build_type must be 'debug' or 'release' (got: ${build_type})" >&2; \
        exit 1 ;; \
    esac

FROM ubuntu:24.04
COPY --from=build /out/mbs4-server /usr/local/bin/mbs4-server
COPY --from=build /out/mbs4-cli /usr/local/bin/mbs4-cli
COPY --from=build /src/test-data/mbs4.db /build-data/mbs4.db
COPY ./container-entrypoint.sh /usr/local/bin/container-entrypoint.sh

ARG calibre_version=8.16.2
# Install Calibre
RUN set -eu && \
    apt-get update && apt-get install -y --no-install-recommends \
    libegl1 libopengl0 libxcb-cursor0 wget ca-certificates python3 libfreetype6 xz-utils \
    libglx0 libxkbcommon0 libfontconfig1 && \
    rm -rf /var/lib/apt/lists/* && \
    wget -nv -O /tmp/calibre-installer.sh https://download.calibre-ebook.com/linux-installer.sh && \
    bash /tmp/calibre-installer.sh version="${calibre_version}" && \
    rm -f /tmp/calibre-installer.sh && \
    ebook-convert --version 
   

EXPOSE 3000
ENTRYPOINT ["/usr/local/bin/container-entrypoint.sh"]