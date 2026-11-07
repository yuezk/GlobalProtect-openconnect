# Public client identity fixtures

These generated keys and certificates are **public test data**, never production
credentials. Passwords are `fixture-password`, except `unicode-password.p12`,
which uses `pässword-密码`, and the `rsa-pbes1-3des-password-{127,128}.pem`
fixtures, which use `x` repeated the indicated number of bytes.

Regenerate intentionally with OpenSSL 3:

```sh
OPENSSL_BIN=/path/to/openssl bash generate.sh
```

Generation is random; it replaces the keys, certificates, and archives together.
The test runner reads checked-in files and needs no converter executable.
No fixture generation is needed to run the tests. OpenSSL is used only by the
optional fixture-generation script, not identity import.

| Files | Contents |
| --- | --- |
| `root.pem`, `rsa-cert.pem`, `ec-cert.pem` | Test root and issued RSA-2048 / P-256 client certificates, SHA256 signatures |
| `rsa-critical-extension.pem` | Self-signed certificate for the existing RSA key with a private critical extension; issue #697 desktop validation regression |
| `rsa-pkcs8.pem`, `ec-pkcs8.pem` | Plain PKCS#8 keys; expected normalized output |
| `rsa-pkcs1.pem`, `ec-sec1.pem` | Same keys in traditional containers |
| `{rsa,ec}-traditional-{aes128,aes192,aes256,des3}.pem` | Traditional encrypted PEM with AES-CBC or 3DES-CBC; password `fixture-password` |
| `rsa-pbes2-sha1.pem`, `rsa-pbes2-sha256.pem` | Encrypted PKCS#8, AES256-CBC/PBKDF2 with SHA1 or SHA256 PRF |
| `rsa-pbes2-aes-{128,192}-cbc-{sha1,sha256}.pem` | AES128/192-CBC with PBKDF2 SHA1/SHA256 |
| `rsa-pbes2-des3-{sha1,sha256}.pem` | PBES2 3DES-CBC with PBKDF2 SHA1/SHA256; distinct from the PKCS#12 PBE scheme handled by GnuTLS |
| `rsa-pbes2-des.pem` | PBES2 DES-CBC/PBKDF2-SHA256; Rust processing requires no native legacy provider |
| `rsa-pbes2-scrypt.pem` | PBES2 AES256-CBC with scrypt |
| `rsa-pbes1-3des-password-{127,128}.pem` | Legacy password boundary: 127 ASCII characters import; 128 report the supported limit |
| `rsa-pbes1-3des.pem` | Legacy PKCS#12 SHA1/3DES scheme inside standalone encrypted PKCS#8; private GnuTLS import case |
| `empty-password.p12` | RSA leaf/key/root with an empty password, accepted with omitted or empty password |
| `ec-pbes2.pem` | Encrypted EC PKCS#8, AES256-CBC/PBKDF2-SHA256 |
| `modern.p12`, `unicode-password.p12` | Leaf/key/root, AES256-CBC/PBKDF2-SHA256, SHA256 MAC; RSA and EC respectively |
| `certificate-only.pfx`, `key-only.pfx` | Incomplete PKCS#12 archives: certificate without key, and key without certificate; password `fixture-password` |
| `legacy-rc2.p12` | Issue #437 case: RC2-40 certificate bags, SHA1/3DES key bag, SHA1 MAC |
| `ec-p521.pem`, `ec-p521-cert.pem` | Valid P-521 identity; explicit unsupported-key error until HTTP negotiation parity exists |
| `rsa-pss.pem`, `rsa-pss-cert.pem` | Valid RSA-PSS identity; explicit unsupported-key error until Rustls negotiation parity exists |
| `pss-chain-{root,issuer}.pem`, `pss-chain-salt-{20,32,48}.pem` | RSA client certificates signed with RSA-PSS/SHA256 and different salt lengths; client key is `rsa-pkcs8.pem`. Regenerate with `generate-pss-chain.sh` |
| `ec-{compressed,explicit}.pem`, matching `-cert.pem` files | Valid OpenSSL inputs rejected by the tested GnuTLS backend; actionable unsupported-encoding tests |
| `legacy-RC4-128.p12` | SHA1/RC4-128 key encryption and SHA1/3DES certificate encryption; native archive import |
| `legacy-RC2-128.p12`, `legacy-2DES.p12` | Unsupported archive encryption; terminal AES re-export guidance with any password |


`tests/client_identity.rs` covers all supported key families, password failures,
archive integrity, certificate/key matching, retained snapshots, private-file
permissions and cleanup. `tests/identity_encoding.rs` covers PEM/DER, RSA PKCS#1,
EC SEC1, traditional encrypted PEM, first-key selection, malformed encryption
headers and archive extension capitalization. Desktop GnuTLS validates and signs client keys; portable builds keep the Rust
implementation. Legacy encrypted-key imports use the private GnuTLS shim.
Legacy PKCS#12 SHA1/3DES keys use a consistent limit of 127 UTF-16 code units
(256 bytes including the terminator), matching older supported GnuTLS releases.
Longer passwords report a clear error without exposing the password.
PBES2, including scrypt, stays in Rust and does not use this limit. The
no-executable child test runs with an empty PATH and imports both legacy key
families and RC2 PKCS#12.

Archive integrity checks change the UTF-16 friendly name without changing the
MAC. Decrypted output is compared with independent plaintext fixture keys.
Unit tests in `utils/request.rs` cover missing keys, large local files and request
client construction.

`tests/client_certificate_extensions.rs` covers issue #697 with generated
RSA, P-256, P-384 and Ed25519 identities: no extensions, private critical and
noncritical extensions, critical Netscape client type, Microsoft template,
key usage and client EKU. Each is exercised as separate PEM, combined PEM,
DER, PKCS#12 and a service snapshot, including snapshot/file round trips and
request-client construction. Independent OpenSSL checks establish local key
compatibility. TLS 1.2/1.3 tests verify client authentication with PEM and
PKCS#12 and confirm the original signed certificate is sent unchanged. Negative
cases retain key mismatch errors, server trust checks and server rejection of
a client certificate. The checked-in critical-extension fixture also covers
the shared certificate validation used by both desktop shells.

`tests/certificate_compatibility.rs` checks RC4-128 import, renewed identities and P-521 rejection,
and typed, actionable errors for unsupported algorithms,
EC encodings and archive encryption. The OpenConnect integration suite checks
the real loader/handshake separately from these HTTP/import tests.

Desktop local matching and signing use GnuTLS. The VPN server validates client
certificate extensions and trust; HTTP server trust still uses the platform
verifier with Rustls/ring. RSA-PSS and P-521 key types deliberately fail before connecting
until the HTTP transport can negotiate them correctly on both TLS versions.
This differs from ordinary RSA keys producing RSA-PSS signatures, which remain
supported. OpenSSL remains development-only.

`tests/client_certificate_chain.rs` checks mutual TLS with PKCS#12 CA rollover,
including self-issued intermediates and both certificate bag orders. It also
checks RSA-PSS certificate signatures with 20-, 32-, and 48-byte salts against
a same-name, wrong-key issuer. The server must receive the original leaf and
correct intermediate; OpenConnect must retain the original archive.

`tests/client_certificate_tls12_hash.rs` records the remaining TLS 1.2 P-384
with SHA-256-only peer request gap: the OpenSSL baseline passes, while the
replacement reproducer is explicitly ignored pending negotiated-version-aware
signing. TLS 1.3 curve restrictions must remain intact.

The unencrypted, MAC-less `no-bag-ids.p12`, `mismatched.p12`, and
`multiple-identities.p12` contain only the same public test keys/certificates.
They exercise PKCS#12 bags without optional attributes, reversed certificate
order/unrelated certificates, and terminal mismatched/ambiguous identities.
