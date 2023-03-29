###################################################################################
## Builder
###################################################################################
FROM rust:latest AS builder

RUN rustup target add x86_64-unknown-linux-musl
RUN apt update && apt install -y musl-tools musl-dev upx pkg-config libssl-dev
RUN update-ca-certificates

# Create appuser
ENV USER=rust
ENV UID=10001

RUN adduser \
  --disabled-password \
  --gecos "" \
  --home "/" \
  --shell "/sbin/nologin" \
  --no-create-home \
  --uid "${UID}" \
  "${USER}"


WORKDIR /workdir

COPY ./ .

RUN chmod -R 444 ./certs

RUN cargo build --target x86_64-unknown-linux-musl --release
RUN upx --best --lzma target/x86_64-unknown-linux-musl/release/push-config-injecter

###################################################################################
## Final image
###################################################################################
FROM scratch

WORKDIR /

# Copy from builder.
COPY --from=builder /workdir/certs /certs
COPY --from=builder /etc/passwd /etc/passwd
COPY --from=builder /etc/group /etc/group
COPY --from=builder /workdir/target/x86_64-unknown-linux-musl/release/push-config-injecter/ /

# Use an unprivileged user.
USER rust:rust

CMD ["./push-config-injecter"]
