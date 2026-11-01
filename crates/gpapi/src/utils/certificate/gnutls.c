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
