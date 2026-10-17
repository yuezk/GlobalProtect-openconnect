# Public client identity fixtures

These generated keys and certificates are **public test data**, never production
credentials. Passwords are `fixture-password`, except `unicode-password.p12`,
which uses `pässword-密码`.

Regenerate intentionally with OpenSSL 3:

```sh
OPENSSL_BIN=/path/to/openssl bash generate.sh
```

Generation is random; it replaces the keys, certificates, and archives together.
The test runner reads checked-in files and does not invoke the OpenSSL CLI.
No fixture generation is needed to run the tests.

| Files | Contents |
| --- | --- |
| `root.pem`, `rsa-cert.pem`, `ec-cert.pem` | Test root and issued RSA-2048 / P-256 client certificates, SHA256 signatures |
| `rsa-pkcs8.pem`, `ec-pkcs8.pem` | Plain PKCS#8 keys; expected normalized output |
| `rsa-pkcs1.pem`, `ec-sec1.pem` | Same keys in traditional containers |
| `rsa-pbes2-sha1.pem`, `rsa-pbes2-sha256.pem` | Encrypted PKCS#8, AES256-CBC/PBKDF2 with SHA1 or SHA256 PRF |
| `rsa-pbes2-aes-{128,192}-cbc-{sha1,sha256}.pem` | AES128/192-CBC with PBKDF2 SHA1/SHA256 |
| `rsa-pbes2-des3-{sha1,sha256}.pem` | PBES2 3DES-CBC with PBKDF2 SHA1/SHA256; distinct from the unsupported legacy PBE scheme |
| `rsa-pbes2-des.pem` | PBES2 DES-CBC/PBKDF2-SHA256; Rust processing requires no native legacy provider |
| `rsa-pbes2-scrypt.pem` | PBES2 AES256-CBC with scrypt |
| `rsa-pbes1-3des.pem` | Legacy PKCS#12 SHA1/3DES scheme inside standalone encrypted PKCS#8; deliberate migration rejection case |
| `empty-password.p12` | RSA leaf/key/root with an empty password, preserving native backend acceptance |
| `ec-pbes2.pem` | Encrypted EC PKCS#8, AES256-CBC/PBKDF2-SHA256 |
| `modern.p12`, `unicode-password.p12` | Leaf/key/root, AES256-CBC/PBKDF2-SHA256, SHA256 MAC; RSA and EC respectively |
| `legacy-rc2.p12` | Issue #437 case: RC2-40 certificate bags, SHA1/3DES key bag, SHA1 MAC |

`tests/client_identity.rs` exercises the existing public loader. It records
current native import restrictions, rather than changing production behavior:
traditional unencrypted key containers fail before normalization; macOS also
rejects the P-256 PKCS#8 PEM fixtures, although its native PKCS#12 import accepts
the EC archive. Legacy-provider setup is isolated in a child process on Linux/BSD.

Standalone legacy PKCS#8 PBE encryption is deliberately rejected with conversion
guidance before prompting for a password. PBES2 DES/3DES remain supported.

The tests check archive integrity by changing the UTF-16 friendly name without
changing the MAC, and compare normalized PEM output against the independent
checked-in plaintext key. Public integration tests cover first-key selection,
retained snapshots, private-file permissions and cleanup, `.pfx`, empty-password
backend behavior, and chain retention. Unit tests in `utils/request.rs` cover
missing keys, large local files, and native request-client construction.
