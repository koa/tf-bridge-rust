FROM docker.io/library/rust:1.75.0 as rust
RUN rustup target add x86_64-unknown-linux-musl
RUN cargo install trunk
RUN apt -y update && apt -y install musl-tools
ADD . build/
RUN cd build; cargo build --release --target x86_64-unknown-linux-musl
FROM scratch as target
COPY --from=rust /etc/ssl /etc/ssl
COPY --from=rust /build/target/x86_64-unknown-linux-musl/release/tf_bridge_rust /
ADD config-prod.yaml /config.yaml
EXPOSE 8080/tcp
ENTRYPOINT ["/tf_bridge_rust"]