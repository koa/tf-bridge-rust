FROM docker.io/library/rust:1.75.0 as rust
FROM scratch as target
COPY --from=rust /etc/ssl /etc/ssl
ADD target/x86_64-unknown-linux-musl/release/tf_bridge_rust /
ADD config-prod.yaml /config.yaml
EXPOSE 8080/tcp
ENTRYPOINT ["/tf_bridge_rust"]