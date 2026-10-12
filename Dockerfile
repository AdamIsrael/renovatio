# A container for building Rust/GTK applications
# Pinned to Debian 13 (trixie) so release binaries keep the same minimum
# glibc/GTK/libadwaita versions (see .github/release-notes.md).
FROM rust:1-trixie AS builder
WORKDIR /app
COPY . .

# Install the GTK development libraries
RUN apt-get update && apt-get install -y libgtk-4-dev libadwaita-1-dev

RUN rustup component add clippy
