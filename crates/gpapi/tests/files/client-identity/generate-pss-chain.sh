#!/usr/bin/env bash
# Public test certificates only; uses the existing public RSA client key.
set -euo pipefail
cd "$(dirname "$0")"
openssl_bin=${OPENSSL_BIN:-openssl}
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
"$openssl_bin" req -x509 -newkey rsa:2048 -noenc -sha256 -days 3650 \
  -subj '/CN=PSS Chain Root' -addext 'basicConstraints=critical,CA:TRUE' \
  -keyout "$scratch/root.key" -out pss-chain-root.pem
"$openssl_bin" req -new -newkey rsa:2048 -noenc -subj '/CN=PSS Issuing CA' \
  -keyout "$scratch/issuer.key" -out "$scratch/issuer.csr"
printf 'basicConstraints=critical,CA:TRUE\nkeyUsage=critical,keyCertSign\n' > "$scratch/ca.ext"
"$openssl_bin" x509 -req -in "$scratch/issuer.csr" -CA pss-chain-root.pem \
  -CAkey "$scratch/root.key" -set_serial 2 -days 3650 -sha256 \
  -extfile "$scratch/ca.ext" -out pss-chain-issuer.pem
"$openssl_bin" req -new -key rsa-pkcs8.pem -subj '/CN=PSS Signed Client' -out "$scratch/client.csr"
printf 'basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=clientAuth\n' > "$scratch/client.ext"
for salt_length in 20 32 48; do
  "$openssl_bin" x509 -req -in "$scratch/client.csr" -CA pss-chain-issuer.pem \
    -CAkey "$scratch/issuer.key" -set_serial "$salt_length" -days 3650 -sha256 \
    -sigopt rsa_padding_mode:pss -sigopt "rsa_pss_saltlen:$salt_length" \
    -extfile "$scratch/client.ext" -out "pss-chain-salt-$salt_length.pem"
done
