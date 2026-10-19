#!/usr/bin/env bash
# Public test identities only. Run with OpenSSL 3; no production credentials.
set -euo pipefail
cd "$(dirname "$0")"
openssl_bin=${OPENSSL_BIN:-openssl}
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

"$openssl_bin" req -x509 -newkey rsa:2048 -noenc -sha256 -days 3650 \
  -subj '/CN=GP Identity Test Root' -addext 'basicConstraints=critical,CA:TRUE' \
  -keyout "$scratch/root.key" -out root.pem
"$openssl_bin" genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out rsa-pkcs8.pem
"$openssl_bin" genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out ec-pkcs8.pem
for algorithm in rsa ec; do
  "$openssl_bin" req -new -key "$algorithm-pkcs8.pem" -subj "/CN=GP Identity Test $algorithm" \
    -out "$scratch/$algorithm.csr"
  "$openssl_bin" x509 -req -in "$scratch/$algorithm.csr" -CA root.pem -CAkey "$scratch/root.key" \
    -set_serial 10 -days 3650 -sha256 -out "$algorithm-cert.pem"
done
"$openssl_bin" rsa -in rsa-pkcs8.pem -traditional -out rsa-pkcs1.pem
"$openssl_bin" ec -in ec-pkcs8.pem -out ec-sec1.pem
for cipher in aes128 aes192 aes256 des3; do
  "$openssl_bin" rsa -in rsa-pkcs8.pem -traditional -"$cipher" \
    -passout pass:fixture-password -out "rsa-traditional-$cipher.pem"
  "$openssl_bin" ec -in ec-pkcs8.pem -"$cipher" \
    -passout pass:fixture-password -out "ec-traditional-$cipher.pem"
done
for prf in sha1 sha256; do
  if [[ "$prf" == sha1 ]]; then
    prf_name=hmacWithSHA1
  else
    prf_name=hmacWithSHA256
  fi
  "$openssl_bin" pkcs8 -topk8 -in rsa-pkcs8.pem -v2 aes-256-cbc -v2prf "$prf_name" \
    -passout pass:fixture-password -out "rsa-pbes2-$prf.pem"
done
for cipher in aes-128-cbc aes-192-cbc des3; do
  for prf in sha1 sha256; do
    if [[ "$prf" == sha1 ]]; then
      prf_name=hmacWithSHA1
    else
      prf_name=hmacWithSHA256
    fi
    "$openssl_bin" pkcs8 -topk8 -in rsa-pkcs8.pem -v2 "$cipher" -v2prf "$prf_name" \
      -passout pass:fixture-password -out "rsa-pbes2-$cipher-$prf.pem"
  done
done
"$openssl_bin" pkcs8 -topk8 -in rsa-pkcs8.pem -scrypt -v2 aes-256-cbc \
  -passout pass:fixture-password -out rsa-pbes2-scrypt.pem
"$openssl_bin" pkcs8 -topk8 -in rsa-pkcs8.pem -v1 PBE-SHA1-3DES \
  -passout pass:fixture-password -out rsa-pbes1-3des.pem
for length in 1023 1024; do
  printf -v password '%*s' "$length" ''
  password=${password// /x}
  "$openssl_bin" pkcs8 -topk8 -in rsa-pkcs8.pem -v1 PBE-SHA1-3DES \
    -passout "pass:$password" -out "rsa-pbes1-3des-password-$length.pem"
done
"$openssl_bin" pkcs8 -topk8 -in ec-pkcs8.pem -v2 aes-256-cbc \
  -passout pass:fixture-password -out ec-pbes2.pem
"$openssl_bin" pkcs12 -export -inkey rsa-pkcs8.pem -in rsa-cert.pem -certfile root.pem \
  -name fixture -passout pass:fixture-password -out modern.p12
"$openssl_bin" pkcs12 -export -legacy -inkey rsa-pkcs8.pem -in rsa-cert.pem -certfile root.pem \
  -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-RC2-40 -macalg SHA1 \
  -name fixture -passout pass:fixture-password -out legacy-rc2.p12
"$openssl_bin" pkcs12 -export -inkey ec-pkcs8.pem -in ec-cert.pem -certfile root.pem \
  -name fixture -passout 'pass:pässword-密码' -out unicode-password.p12
"$openssl_bin" pkcs12 -export -inkey rsa-pkcs8.pem -in rsa-cert.pem -certfile root.pem \
  -name fixture -passout pass: -out empty-password.p12
# DES requires the legacy provider in OpenSSL 3, unlike PBES2 3DES.
"$openssl_bin" pkcs8 -provider default -provider legacy -topk8 -in rsa-pkcs8.pem \
  -v2 des-cbc -v2prf hmacWithSHA256 -passout pass:fixture-password -out rsa-pbes2-des.pem

# Incomplete identity archives exercise validation before native TLS import.
"$openssl_bin" pkcs12 -export -nokeys -in rsa-cert.pem \
  -name fixture -passout pass:fixture-password -out certificate-only.pfx
"$openssl_bin" pkcs12 -export -nocerts -inkey rsa-pkcs8.pem \
  -name fixture -passout pass:fixture-password -out key-only.pfx
