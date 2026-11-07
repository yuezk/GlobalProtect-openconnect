#include <gnutls/gnutls.h>
#include <gnutls/x509.h>
#include <limits.h>
#include <stddef.h>

/* Only decrypted PKCS#8 leaves this interface. The Rust owner releases it with
 * gp_key_free, so GnuTLS allocation and wiping stay with the native allocator. */
int gp_key_import(const unsigned char *input, size_t length, int pem,
                  const char *password, unsigned char **output, size_t *output_length) {
  gnutls_x509_privkey_t key = NULL;
  gnutls_datum_t exported = {NULL, 0};
  int result;
  *output = NULL;
  *output_length = 0;
  if (length > UINT_MAX) return 3;
  result = gnutls_global_init();
  if (result < 0) return 3;
  result = gnutls_x509_privkey_init(&key);
  if (result >= 0) {
    gnutls_datum_t data = {(unsigned char *)input, (unsigned int)length};
    result = gnutls_x509_privkey_import2(key, &data,
        pem ? GNUTLS_X509_FMT_PEM : GNUTLS_X509_FMT_DER, password, 0);
  }
  if (result >= 0) {
    result = gnutls_x509_privkey_export2_pkcs8(key, GNUTLS_X509_FMT_DER,
                                            NULL, GNUTLS_PKCS_PLAIN, &exported);
  }
  if (key != NULL) gnutls_x509_privkey_deinit(key);
  gnutls_global_deinit();
  if (result >= 0) {
    *output = exported.data;
    *output_length = exported.size;
    return 0;
  }
  if (result == GNUTLS_E_DECRYPTION_FAILED) return 1;
  if (result == GNUTLS_E_UNKNOWN_CIPHER_TYPE || result == GNUTLS_E_UNIMPLEMENTED_FEATURE)
    return 2;
  return 3;
}

void gp_key_free(unsigned char *data, size_t length) {
  if (data != NULL) {
    gnutls_memset(data, 0, length);
    gnutls_free(data);
  }
}

#include <gnutls/abstract.h>
#include <stdlib.h>
#include <string.h>

/* Check only the signature, not whether this local certificate is trusted,
 * current, or has extensions understood by the client. */
int gp_certificate_verify_signature(
    const unsigned char *certificate, size_t certificate_length,
    const unsigned char *issuer_key, size_t issuer_key_length,
    const unsigned char *message, size_t message_length,
    const unsigned char *signature, size_t signature_length) {
  gnutls_x509_crt_t child = NULL;
  gnutls_pubkey_t public_key = NULL;
  int result, algorithm;
  if (certificate_length > UINT_MAX || issuer_key_length > UINT_MAX ||
      message_length > UINT_MAX || signature_length > UINT_MAX) return -1;
  result = gnutls_global_init();
  if (result < 0) return -1;
  gnutls_datum_t child_data = {(unsigned char *)certificate, (unsigned int)certificate_length};
  gnutls_datum_t key_data = {(unsigned char *)issuer_key, (unsigned int)issuer_key_length};
  gnutls_datum_t data = {(unsigned char *)message, (unsigned int)message_length};
  gnutls_datum_t sig = {(unsigned char *)signature, (unsigned int)signature_length};
  result = gnutls_x509_crt_init(&child);
  if (result >= 0) result = gnutls_x509_crt_import(child, &child_data, GNUTLS_X509_FMT_DER);
  if (result >= 0) result = gnutls_pubkey_init(&public_key);
  if (result >= 0) result = gnutls_pubkey_import(public_key, &key_data, GNUTLS_X509_FMT_DER);
  if (result >= 0) {
    algorithm = gnutls_x509_crt_get_signature_algorithm(child);
    if (algorithm < 0) result = algorithm;
    else result = gnutls_pubkey_verify_data2(public_key, algorithm,
        GNUTLS_VERIFY_ALLOW_BROKEN | GNUTLS_VERIFY_ALLOW_SIGN_WITH_SHA1, &data, &sig) >= 0;
  }
  if (public_key != NULL) gnutls_pubkey_deinit(public_key);
  if (child != NULL) gnutls_x509_crt_deinit(child);
  gnutls_global_deinit();
  return result < 0 ? -1 : result;
}

/* Stable private kinds, independent of the native enum values. */
struct gp_client_key {
  gnutls_privkey_t signer;
  gnutls_pubkey_t public_key;
};

void gp_client_key_delete(struct gp_client_key *key) {
  if (key == NULL) return;
  if (key->public_key != NULL) gnutls_pubkey_deinit(key->public_key);
  if (key->signer != NULL) gnutls_privkey_deinit(key->signer);
  free(key);
  gnutls_global_deinit();
}

int gp_client_key_new(const unsigned char *data, size_t length, int *kind,
                      struct gp_client_key **output) {
  gnutls_x509_privkey_t input = NULL;
  struct gp_client_key *key = NULL;
  int result, algorithm;
  *output = NULL;
  *kind = 9;
  if (length > UINT_MAX || gnutls_global_init() < 0) return 1;
  key = calloc(1, sizeof(*key));
  if (key == NULL) { gnutls_global_deinit(); return 1; }
  result = gnutls_x509_privkey_init(&input);
  if (result >= 0) {
    gnutls_datum_t bytes = {(unsigned char *)data, (unsigned int)length};
    result = gnutls_x509_privkey_import2(input, &bytes, GNUTLS_X509_FMT_DER, NULL, 0);
  }
  if (result < 0) goto invalid;
  algorithm = gnutls_x509_privkey_get_pk_algorithm(input);
  if (algorithm == GNUTLS_PK_RSA_PSS) { *kind = 5; goto unsupported; }
  if (algorithm == GNUTLS_PK_DSA) { *kind = 7; goto unsupported; }
  if (algorithm == GNUTLS_PK_EDDSA_ED448) { *kind = 8; goto unsupported; }
  result = gnutls_privkey_init(&key->signer);
  if (result < 0) goto invalid;
  result = gnutls_privkey_import_x509(key->signer, input, GNUTLS_PRIVKEY_IMPORT_COPY);
  if (result < 0) goto invalid;
  result = gnutls_pubkey_init(&key->public_key);
  if (result < 0) goto invalid;
  result = gnutls_pubkey_import_privkey(key->public_key, key->signer, 0, 0);
  if (result < 0) goto invalid;
  if (algorithm == GNUTLS_PK_RSA) *kind = 0;
  else if (algorithm == GNUTLS_PK_EDDSA_ED25519) *kind = 4;
  else if (algorithm == GNUTLS_PK_ECDSA) {
    gnutls_ecc_curve_t curve;
    gnutls_datum_t x = {NULL, 0}, y = {NULL, 0};
    result = gnutls_pubkey_export_ecc_raw(key->public_key, &curve, &x, &y);
    gnutls_free(x.data);
    gnutls_free(y.data);
    if (result < 0) goto invalid;
    if (curve == GNUTLS_ECC_CURVE_SECP256R1) *kind = 1;
    else if (curve == GNUTLS_ECC_CURVE_SECP384R1) *kind = 2;
    else if (curve == GNUTLS_ECC_CURVE_SECP521R1) { *kind = 3; goto unsupported; }
    else { *kind = 6; goto unsupported; }
  } else goto unsupported;
  gnutls_x509_privkey_deinit(input);
  *output = key;
  return 0;
unsupported:
  if (input != NULL) gnutls_x509_privkey_deinit(input);
  gp_client_key_delete(key);
  return 2;
invalid:
  if (input != NULL) gnutls_x509_privkey_deinit(input);
  gp_client_key_delete(key);
  return 1;
}

int gp_client_key_public(struct gp_client_key *key, unsigned char **output, size_t *length) {
  gnutls_datum_t bytes = {NULL, 0};
  int result = gnutls_pubkey_export2(key->public_key, GNUTLS_X509_FMT_DER, &bytes);
  *output = bytes.data;
  *length = bytes.size;
  return result;
}

int gp_client_key_matches(struct gp_client_key *key, const unsigned char *data, size_t length) {
  gnutls_x509_crt_t certificate = NULL;
  gnutls_pubkey_t public_key = NULL;
  unsigned char key_id[32], cert_id[32];
  size_t key_id_length = sizeof(key_id), cert_id_length = sizeof(cert_id);
  int result;
  if (length > UINT_MAX) return -1;
  result = gnutls_x509_crt_init(&certificate);
  if (result >= 0) {
    gnutls_datum_t bytes = {(unsigned char *)data, (unsigned int)length};
    result = gnutls_x509_crt_import(certificate, &bytes, GNUTLS_X509_FMT_DER);
  }
  if (result >= 0) result = gnutls_pubkey_init(&public_key);
  if (result >= 0) result = gnutls_pubkey_import_x509(public_key, certificate, 0);
  if (result >= 0) result = gnutls_pubkey_get_key_id(key->public_key, GNUTLS_KEYID_USE_SHA256, key_id, &key_id_length);
  if (result >= 0) result = gnutls_pubkey_get_key_id(public_key, GNUTLS_KEYID_USE_SHA256, cert_id, &cert_id_length);
  if (public_key != NULL) gnutls_pubkey_deinit(public_key);
  if (certificate != NULL) gnutls_x509_crt_deinit(certificate);
  if (result < 0) return result;
  return key_id_length == cert_id_length && !memcmp(key_id, cert_id, key_id_length);
}

int gp_client_key_sign(struct gp_client_key *key, unsigned short scheme,
                       const unsigned char *message, size_t length,
                       unsigned char **output, size_t *output_length) {
  gnutls_sign_algorithm_t algorithm;
  unsigned int flags = 0;
  *output = NULL;
  *output_length = 0;
  if (length > UINT_MAX) return -1;
  switch (scheme) {
    case 0x0804: algorithm = GNUTLS_SIGN_RSA_PSS_RSAE_SHA256; break;
    case 0x0805: algorithm = GNUTLS_SIGN_RSA_PSS_RSAE_SHA384; break;
    case 0x0806: algorithm = GNUTLS_SIGN_RSA_PSS_RSAE_SHA512; break;
    case 0x0401: algorithm = GNUTLS_SIGN_RSA_SHA256; break;
    case 0x0501: algorithm = GNUTLS_SIGN_RSA_SHA384; break;
    case 0x0601: algorithm = GNUTLS_SIGN_RSA_SHA512; break;
    case 0x0403: algorithm = GNUTLS_SIGN_ECDSA_SECP256R1_SHA256; break;
    case 0x0503: algorithm = GNUTLS_SIGN_ECDSA_SECP384R1_SHA384; break;
    case 0x0807: algorithm = GNUTLS_SIGN_EDDSA_ED25519; break;
    default: return -1;
  }
  if (scheme >= 0x0804 && scheme <= 0x0806)
    flags = GNUTLS_PRIVKEY_FLAG_RSA_PSS_FIXED_SALT_LENGTH;
  gnutls_datum_t bytes = {(unsigned char *)message, (unsigned int)length};
  gnutls_datum_t signature = {NULL, 0};
  int result = gnutls_privkey_sign_data2(key->signer, algorithm, flags, &bytes, &signature);
  *output = signature.data;
  *output_length = signature.size;
  return result;
}

#include <gnutls/pkcs12.h>
#include <stdint.h>

/* Invoked only after Rust has identified the supported RC4 archive scheme.
 * Integrity and identity-count checks precede the native leaf selection. */
int gp_pkcs12_identity(const unsigned char *input, size_t length, const char *password,
                       unsigned char **certificate, size_t *certificate_length,
                       unsigned char **private_key, size_t *private_key_length) {
  gnutls_pkcs12_t archive = NULL;
  gnutls_x509_privkey_t key = NULL;
  gnutls_x509_crt_t *chain = NULL, *extra = NULL;
  unsigned int chain_length = 0, extra_length = 0;
  gnutls_datum_t exported_key = {NULL, 0};
  unsigned int key_count = 0, cert_count = 0;
  int result, status = 5;
  *certificate = NULL;
  *certificate_length = 0;
  *private_key = NULL;
  *private_key_length = 0;
  if (length > UINT_MAX || gnutls_global_init() < 0) return 5;
  result = gnutls_pkcs12_init(&archive);
  if (result >= 0) {
    gnutls_datum_t bytes = {(unsigned char *)input, (unsigned int)length};
    result = gnutls_pkcs12_import(archive, &bytes, GNUTLS_X509_FMT_DER, 0);
  }
  if (result < 0) goto out;
  result = gnutls_pkcs12_verify_mac(archive, password);
  if (result == GNUTLS_E_MAC_VERIFY_FAILED) { status = 1; goto out; }
  if (result < 0 && result != GNUTLS_E_REQUESTED_DATA_NOT_AVAILABLE) goto out;
  for (int index = 0; ; index++) {
    gnutls_pkcs12_bag_t bag;
    result = gnutls_pkcs12_bag_init(&bag);
    if (result < 0) goto out;
    result = gnutls_pkcs12_get_bag(archive, index, bag);
    if (result == GNUTLS_E_REQUESTED_DATA_NOT_AVAILABLE) {
      gnutls_pkcs12_bag_deinit(bag);
      break;
    }
    if (result >= 0 && gnutls_pkcs12_bag_get_type(bag, 0) == GNUTLS_BAG_ENCRYPTED)
      result = gnutls_pkcs12_bag_decrypt(bag, password);
    if (result < 0) {
      gnutls_pkcs12_bag_deinit(bag);
      if (result == GNUTLS_E_DECRYPTION_FAILED) status = 1;
      else if (result == GNUTLS_E_UNKNOWN_CIPHER_TYPE || result == GNUTLS_E_UNIMPLEMENTED_FEATURE) status = 4;
      goto out;
    }
    int count = gnutls_pkcs12_bag_get_count(bag);
    if (count < 0) { gnutls_pkcs12_bag_deinit(bag); goto out; }
    for (int item = 0; item < count; item++) {
      int type = gnutls_pkcs12_bag_get_type(bag, item);
      if (type == GNUTLS_BAG_PKCS8_KEY || type == GNUTLS_BAG_PKCS8_ENCRYPTED_KEY) key_count++;
      if (type == GNUTLS_BAG_CERTIFICATE) cert_count++;
    }
    gnutls_pkcs12_bag_deinit(bag);
  }
  if (key_count == 0 || cert_count == 0) { status = 2; goto out; }
  if (key_count != 1) { status = 3; goto out; }
  result = gnutls_pkcs12_simple_parse(archive, password, &key, &chain, &chain_length,
                                    &extra, &extra_length, NULL, GNUTLS_PKCS12_SP_INCLUDE_SELF_SIGNED);
  if (result < 0) {
    if (result == GNUTLS_E_DECRYPTION_FAILED) status = 1;
    else if (result == GNUTLS_E_UNKNOWN_CIPHER_TYPE || result == GNUTLS_E_UNIMPLEMENTED_FEATURE) status = 4;
    else if (result == GNUTLS_E_REQUESTED_DATA_NOT_AVAILABLE) status = 6;
    goto out;
  }
  if (key == NULL || chain_length == 0) { status = 2; goto out; }
  result = gnutls_x509_privkey_export2_pkcs8(key, GNUTLS_X509_FMT_DER, NULL, GNUTLS_PKCS_PLAIN, &exported_key);
  if (result < 0) goto out;
  for (unsigned int index = 0; index < chain_length; index++) {
    gnutls_datum_t part = {NULL, 0};
    result = gnutls_x509_crt_export2(chain[index], GNUTLS_X509_FMT_PEM, &part);
    if (result < 0) goto out;
    if (part.size > SIZE_MAX - *certificate_length) { gnutls_free(part.data); goto out; }
    unsigned char *buffer = gnutls_realloc(*certificate, *certificate_length + part.size);
    if (buffer == NULL) { gnutls_free(part.data); goto out; }
    memcpy(buffer + *certificate_length, part.data, part.size);
    *certificate = buffer;
    *certificate_length += part.size;
    gnutls_free(part.data);
  }
  *private_key = exported_key.data;
  *private_key_length = exported_key.size;
  exported_key.data = NULL;
  exported_key.size = 0;
  status = 0;
out:
  if (status != 0) {
    gp_key_free(*certificate, *certificate_length);
    *certificate = NULL;
    *certificate_length = 0;
  }
  gp_key_free(exported_key.data, exported_key.size);
  if (key != NULL) gnutls_x509_privkey_deinit(key);
  for (unsigned int index = 0; index < chain_length; index++) gnutls_x509_crt_deinit(chain[index]);
  for (unsigned int index = 0; index < extra_length; index++) gnutls_x509_crt_deinit(extra[index]);
  gnutls_free(chain);
  gnutls_free(extra);
  if (archive != NULL) gnutls_pkcs12_deinit(archive);
  gnutls_global_deinit();
  return status;
}
