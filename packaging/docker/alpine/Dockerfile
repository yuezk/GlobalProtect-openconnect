# syntax=docker/dockerfile:1

ARG ALPINE_VERSION=3.23
ARG RUST_VERSION=1.89

FROM rust:${RUST_VERSION}-alpine AS builder

WORKDIR /src

RUN apk add --no-cache \
    autoconf \
    automake \
    build-base \
    file \
    gettext-dev \
    gmp-dev \
    gnutls-dev \
    libtool \
    libxml2-dev \
    linux-headers \
    lz4-dev \
    make \
    nettle-dev \
    openssl-dev \
    p11-kit-dev \
    patch \
    pkgconf \
    zlib-dev

COPY . .

RUN RUSTFLAGS="-C target-feature=-crt-static" \
    make build \
    BUILD_GUI_HELPER=0 \
    INCLUDE_GUI=0 \
    LIBXML2_STATIC=1

RUN install -Dm755 target/release/gpclient /out/usr/bin/gpclient \
    && install -Dm755 target/release/gpauth /out/usr/bin/gpauth \
    && install -Dm755 packaging/files/usr/libexec/gpclient/vpnc-script /out/usr/libexec/gpclient/vpnc-script \
    && install -Dm755 packaging/files/usr/libexec/gpclient/hipreport.sh /out/usr/libexec/gpclient/hipreport.sh

FROM alpine:${ALPINE_VERSION}

ENV SUDO_UID=1000 \
    HOME=/root

RUN apk add --no-cache \
    ca-certificates \
    gmp \
    gnutls \
    iproute2 \
    iputils \
    libgcc \
    libxml2 \
    lz4-libs \
    nettle \
    openssl \
    p11-kit \
    zlib \
    && addgroup -g 1000 gp \
    && adduser -D -u 1000 -G gp gp \
    && update-ca-certificates

COPY --from=builder /out/ /

ENTRYPOINT ["gpclient"]
CMD ["--help"]
