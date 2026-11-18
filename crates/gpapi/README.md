# gpapi tests

Ordinary tests do not require OpenSSL. To run the Windows logger regression test:

```sh
cargo test -p gpapi --features logger --lib logger::windows::tests
```

The certificate interoperability suite uses OpenSSL as an independent TLS and
signature implementation. Full CI enables it explicitly:

```sh
cargo test -p gpapi --features logger,openssl-tests
```

This optional suite requires OpenSSL development headers and libraries.
