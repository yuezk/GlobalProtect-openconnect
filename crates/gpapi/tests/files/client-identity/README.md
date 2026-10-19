# Public client identity fixtures

These generated keys and certificates are **public test data**, never production
credentials. Passwords are `fixture-password`, except `unicode-password.p12`,
which uses `pässword-密码`, and the `rsa-pbes1-3des-password-{1023,1024}.pem`
fixtures, which use `x` repeated the indicated number of bytes.

Regenerate intentionally with OpenSSL 3:

```sh
OPENSSL_BIN=/path/to/openssl bash generate.sh
```

Generation is random; it replaces the keys, certificates, and archives together.
The test runner reads checked-in files. Legacy standalone conversion tests
require an `openssl` executable on PATH (OpenSSL or compatible LibreSSL).
No fixture generation is needed to run the tests.

| Files | Contents |
| --- | --- |
| `root.pem`, `rsa-cert.pem`, `ec-cert.pem` | Test root and issued RSA-2048 / P-256 client certificates, SHA256 signatures |
| `rsa-pkcs8.pem`, `ec-pkcs8.pem` | Plain PKCS#8 keys; expected normalized output |
| `rsa-pkcs1.pem`, `ec-sec1.pem` | Same keys in traditional containers |
| `{rsa,ec}-traditional-{aes128,aes192,aes256,des3}.pem` | Traditional encrypted PEM with AES-CBC or 3DES-CBC; password `fixture-password` |
| `rsa-pbes2-sha1.pem`, `rsa-pbes2-sha256.pem` | Encrypted PKCS#8, AES256-CBC/PBKDF2 with SHA1 or SHA256 PRF |
| `rsa-pbes2-aes-{128,192}-cbc-{sha1,sha256}.pem` | AES128/192-CBC with PBKDF2 SHA1/SHA256 |
| `rsa-pbes2-des3-{sha1,sha256}.pem` | PBES2 3DES-CBC with PBKDF2 SHA1/SHA256; distinct from the legacy PBE scheme handled by optional conversion |
| `rsa-pbes2-des.pem` | PBES2 DES-CBC/PBKDF2-SHA256; Rust processing requires no native legacy provider |
| `rsa-pbes2-scrypt.pem` | PBES2 AES256-CBC with scrypt |
| `rsa-pbes1-3des-password-{1023,1024}.pem` | Same legacy encryption with passwords at the CLI input boundary; the linked importer accepts 1024 bytes, while the converter reports its 1023-byte limit explicitly |
| `rsa-pbes1-3des.pem` | Legacy PKCS#12 SHA1/3DES scheme inside standalone encrypted PKCS#8; optional OpenSSL executable conversion case |
| `empty-password.p12` | RSA leaf/key/root with an empty password, preserving native backend acceptance |
| `ec-pbes2.pem` | Encrypted EC PKCS#8, AES256-CBC/PBKDF2-SHA256 |
| `modern.p12`, `unicode-password.p12` | Leaf/key/root, AES256-CBC/PBKDF2-SHA256, SHA256 MAC; RSA and EC respectively |
| `certificate-only.pfx`, `key-only.pfx` | Incomplete PKCS#12 archives: certificate without key, and key without certificate; password `fixture-password` |
| `legacy-rc2.p12` | Issue #437 case: RC2-40 certificate bags, SHA1/3DES key bag, SHA1 MAC |

`tests/client_identity.rs` records native import compatibility and the owned
identity lifecycle. `tests/identity_encoding.rs` covers container normalization:
DER certificates and keys, RSA PKCS#1, Linux/BSD EC SEC1 and traditional encrypted
PEM, first-key selection, malformed encryption headers, and archive extension
capitalization. macOS rejects SEC1 and traditional encrypted PEM before password
handling. Its native importer also rejects the P-256 PKCS#8 PEM fixture, although
its native PKCS#12 import accepts the EC archive. Legacy-provider setup is isolated
in a child process on Linux/BSD.

Standalone PKCS#8 PBE-SHA1-3DES uses optional OpenSSL executable conversion,
with missing/wrong passwords preserving the old Linux prompt/retry contract.
Other unsupported schemes retain explicit conversion guidance. PBES2 DES/3DES
use Rust processing and need no executable. Child-process tests isolate PATH
for absent/unsupported/stalled converters, invalid output, and secret-free errors.
Timeout coverage includes wrappers whose descendants retain pipes after the
wrapper exits or is killed; all pipe I/O shares the process deadline.
Linux/BSD additionally compare the result with the former linked OpenSSL importer.

The tests check archive integrity by changing the UTF-16 friendly name without
changing the MAC, and compare normalized PEM output against the independent
checked-in plaintext key. Public integration tests cover first-key selection,
retained snapshots, private-file permissions and cleanup, `.pfx`, empty-password
backend behavior, and chain retention. Unit tests in `utils/request.rs` cover
missing keys, large local files, and native request-client construction.
