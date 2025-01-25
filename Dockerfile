# Build image
# Necessary dependencies to build Parrot
FROM rust:1.74.0-slim-bookworm AS build

RUN apt-get update && apt-get install -y \
    build-essential autoconf automake cmake libtool libssl-dev pkg-config wget

RUN wget https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp -O /usr/local/bin/yt-dlp

WORKDIR "/parrot"

# Cache cargo build dependencies by creating a dummy source
RUN mkdir src
RUN echo "fn main() {}" > src/main.rs
COPY Cargo.toml ./
COPY Cargo.lock ./
RUN cargo build --release --locked

COPY . .
RUN cargo build --release --locked

# Release image
# Necessary dependencies to run Parrot
FROM debian:bookworm-slim

RUN apt-get update && apt-get install ffmpeg -y
COPY --from=build /usr/local/bin/yt-dlp /usr/local/bin/yt-dlp
RUN chmod a+rx /usr/local/bin/yt-dlp 

COPY --from=build /parrot/target/release/parrot .

CMD ["./parrot"]
