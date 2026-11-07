/* Exercise the normal OpenConnect setter and GlobalProtect HTTPS path. The peer
 * verifies client TLS authentication, then returns 503: no VPN is configured. */
#include <config.h>
#include "openconnect-internal.h"
#include <gnutls/gnutls.h>
#include <gnutls/pkcs12.h>
#include <assert.h>
#include <limits.h>
#include <pthread.h>
#include <signal.h>
#include <stdarg.h>
#include <time.h>

#define PRIVATE_EXTENSION "1.3.6.1.4.1.55555.1"
#define PASSWORD "fixture-password"

struct peer {
	int listener, tls13, verified;
	gnutls_certificate_credentials_t credentials;
	gnutls_datum_t expected;
};

static void path(char *output, const char *directory, const char *name)
{
	assert(snprintf(output, PATH_MAX, "%s/%s", directory, name) < PATH_MAX);
}

static gnutls_datum_t read_file(const char *filename)
{
	gnutls_datum_t data;
	FILE *file = fopen(filename, "rb");
	long length;
	assert(file);
	assert(!fseek(file, 0, SEEK_END));
	length = ftell(file);
	assert(length > 0 && length <= UINT_MAX);
	assert(!fseek(file, 0, SEEK_SET));
	data.size = length;
	data.data = malloc(data.size);
	assert(data.data && fread(data.data, 1, data.size, file) == data.size);
	assert(!fclose(file));
	return data;
}

static gnutls_x509_crt_t read_certificate(const char *filename)
{
	gnutls_x509_crt_t certificate;
	gnutls_datum_t data = read_file(filename);
	assert(!gnutls_x509_crt_init(&certificate));
	assert(!gnutls_x509_crt_import(certificate, &data, GNUTLS_X509_FMT_PEM));
	free(data.data);
	return certificate;
}

static void progress(void *data, int level, const char *format, ...)
{
	va_list arguments;
	if (level > PRG_DEBUG) return;
	va_start(arguments, format);
	vfprintf(stderr, format, arguments);
	va_end(arguments);
}

static int reject_untrusted_server(void *data, const char *reason)
{
	fprintf(stderr, "Unexpected server verification failure: %s\n", reason);
	return -EINVAL;
}

static int cancel_password_prompt(void *data, struct oc_auth_form *form)
{
	/* A missing/wrong fixture password must fail, never block on input. */
	return 1;
}

static int verify_client(gnutls_session_t session)
{
	struct peer *peer = gnutls_session_get_ptr(session);
	unsigned int count, status;
	const gnutls_datum_t *certificates = gnutls_certificate_get_peers(session, &count);
	assert(certificates && count > 0);
	assert(certificates[0].size == peer->expected.size);
	assert(!memcmp(certificates[0].data, peer->expected.data, peer->expected.size));
	assert(!gnutls_certificate_verify_peers2(session, &status));
	assert(!status);
	peer->verified = 1;
	return 0;
}

static void *serve(void *data)
{
	struct peer *peer = data;
	gnutls_session_t session;
	fd_set reads;
	struct timeval wait = { .tv_sec = 8 };
	struct timeval timeout = { .tv_sec = 5 };
	char request[8192];
	static const char response[] =
		"HTTP/1.1 503 TLS fixture complete\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
	int fd;
	FD_ZERO(&reads);
	FD_SET(peer->listener, &reads);
	assert(select(peer->listener + 1, &reads, NULL, NULL, &wait) == 1);
	fd = accept(peer->listener, NULL, NULL);
	assert(fd >= 0);
	assert(!setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout)));
	assert(!setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof(timeout)));
	assert(!gnutls_init(&session, GNUTLS_SERVER));
	assert(!gnutls_priority_set_direct(session, peer->tls13
		? "NORMAL:-VERS-ALL:+VERS-TLS1.3" : "NORMAL:-VERS-ALL:+VERS-TLS1.2", NULL));
	assert(!gnutls_credentials_set(session, GNUTLS_CRD_CERTIFICATE, peer->credentials));
	gnutls_certificate_server_set_request(session, GNUTLS_CERT_REQUIRE);
	gnutls_session_set_ptr(session, peer);
	gnutls_transport_set_int(session, fd);
	gnutls_handshake_set_timeout(session, 5000);
	/* A successful handshake also verifies CertificateVerify proof of possession. */
	assert(!gnutls_handshake(session));
	assert(peer->verified);
	assert(gnutls_protocol_get_version(session) == (peer->tls13 ? GNUTLS_TLS1_3 : GNUTLS_TLS1_2));
	assert(gnutls_record_recv(session, request, sizeof(request)) > 0);
	assert(gnutls_record_send(session, response, sizeof(response) - 1) == sizeof(response) - 1);
	gnutls_deinit(session);
	close(fd);
	return NULL;
}

static void check(const char *name, const char *server_cert, const char *server_key,
	const char *server_ca, const char *client_ca, const char *certificate,
	const char *key, const char *password, gnutls_x509_crt_t expected,
	int private_extension, int tls13)
{
	struct peer peer = { .tls13 = tls13 };
	struct sockaddr_in address = { .sin_family = AF_INET, .sin_addr.s_addr = htonl(INADDR_LOOPBACK) };
	socklen_t address_length = sizeof(address);
	struct openconnect_info *client;
	pthread_t thread;
	char url[128];
	int result;
	assert(!gnutls_certificate_allocate_credentials(&peer.credentials));
	assert(!gnutls_certificate_set_x509_key_file(peer.credentials, server_cert, server_key, GNUTLS_X509_FMT_PEM));
	assert(gnutls_certificate_set_x509_trust_file(peer.credentials, client_ca, GNUTLS_X509_FMT_PEM) > 0);
	assert(gnutls_certificate_set_x509_trust(peer.credentials, &expected, 1) > 0);
	assert(!gnutls_x509_crt_export2(expected, GNUTLS_X509_FMT_DER, &peer.expected));
	if (private_extension) {
		unsigned critical = 0;
		unsigned char value[2];
		size_t length = sizeof(value);
		assert(!gnutls_x509_crt_get_extension_by_oid(expected, PRIVATE_EXTENSION, 0,
			value, &length, &critical));
		assert(critical && length == 2 && value[0] == 5 && value[1] == 0);
		/* The exact-DER pin in verify_client limits this exception to the known
		 * fixture extension. Chain trust and handshake signatures stay checked. */
		gnutls_certificate_set_verify_flags(peer.credentials, GNUTLS_VERIFY_IGNORE_UNKNOWN_CRIT_EXTENSIONS);
	}
	gnutls_certificate_set_verify_function(peer.credentials, verify_client);
	peer.listener = socket(AF_INET, SOCK_STREAM, 0);
	assert(peer.listener >= 0);
	assert(!bind(peer.listener, (struct sockaddr *)&address, sizeof(address)));
	assert(!getsockname(peer.listener, (struct sockaddr *)&address, &address_length));
	assert(!listen(peer.listener, 1));
	assert(!pthread_create(&thread, NULL, serve, &peer));
	client = openconnect_vpninfo_new("Client identity regression", reject_untrusted_server,
		NULL, cancel_password_prompt, progress, NULL);
	assert(client);
	snprintf(url, sizeof(url), "https://localhost:%u/", ntohs(address.sin_port));
	assert(!openconnect_parse_url(client, url));
	assert(!openconnect_set_protocol(client, "gp"));
	assert(!openconnect_set_cafile(client, server_ca));
	assert(!openconnect_set_client_cert(client, certificate, key));
	if (password) assert(!openconnect_set_key_password(client, password));
	assert(!openconnect_set_cookie(client, "user=fixture&authcookie=fixture"));
	result = openconnect_make_cstp_connection(client);
	assert(result < 0); /* The fixture ends after TLS, without GP configuration. */
	openconnect_vpninfo_free(client);
	assert(!pthread_join(thread, NULL));
	assert(peer.verified);
	close(peer.listener);
	gnutls_free(peer.expected.data);
	gnutls_certificate_free_credentials(peer.credentials);
	printf("%s TLS1.%d: exact client leaf, trusted server and mutual TLS passed\n", name, tls13 ? 3 : 2);
}

static gnutls_x509_crt_t renewed_certificate(gnutls_x509_privkey_t key)
{
	gnutls_x509_crt_t certificate;
	unsigned char serial[] = { 0x65 };
	time_t now = time(NULL);
	assert(!gnutls_x509_crt_init(&certificate));
	assert(!gnutls_x509_crt_set_version(certificate, 3));
	assert(!gnutls_x509_crt_set_dn(certificate, "CN=Public renewed identity", NULL));
	assert(!gnutls_x509_crt_set_issuer_dn(certificate, "CN=Public renewed identity", NULL));
	assert(!gnutls_x509_crt_set_serial(certificate, serial, sizeof(serial)));
	assert(!gnutls_x509_crt_set_key(certificate, key));
	assert(!gnutls_x509_crt_set_activation_time(certificate, now - 3600));
	assert(!gnutls_x509_crt_set_expiration_time(certificate, now + 86400));
	assert(!gnutls_x509_crt_set_basic_constraints(certificate, 1, -1));
	assert(!gnutls_x509_crt_sign2(certificate, certificate, key, GNUTLS_DIG_SHA256, 0));
	return certificate;
}

static void renewal_archive(const char *filename, gnutls_x509_privkey_t key,
	gnutls_x509_crt_t older, gnutls_x509_crt_t renewed, gnutls_x509_crt_t root, int reverse)
{
	gnutls_pkcs12_t archive;
	gnutls_pkcs12_bag_t certificates, keys;
	gnutls_datum_t output, id = { (unsigned char *)"renewed", 7 };
	FILE *file;
	int renewed_index = reverse ? 1 : 0;
	assert(!gnutls_pkcs12_init(&archive));
	assert(!gnutls_pkcs12_bag_init(&certificates));
	assert(gnutls_pkcs12_bag_set_crt(certificates, reverse ? older : renewed) == 0);
	assert(gnutls_pkcs12_bag_set_crt(certificates, reverse ? renewed : older) == 1);
	assert(gnutls_pkcs12_bag_set_crt(certificates, root) == 2);
	/* Keep the bag association on the renewed leaf in both permutations. The
	 * baseline nevertheless selects the first certificate matching the key. */
	assert(!gnutls_pkcs12_bag_set_key_id(certificates, renewed_index, &id));
	assert(!gnutls_pkcs12_set_bag(archive, certificates));
	assert(!gnutls_pkcs12_bag_init(&keys));
	assert(gnutls_pkcs12_bag_set_privkey(keys, key, NULL, GNUTLS_PKCS_PLAIN) == 0);
	assert(!gnutls_pkcs12_bag_set_key_id(keys, 0, &id));
	assert(!gnutls_pkcs12_set_bag(archive, keys));
	assert(!gnutls_pkcs12_generate_mac2(archive, GNUTLS_MAC_SHA256, PASSWORD));
	assert(!gnutls_pkcs12_export2(archive, GNUTLS_X509_FMT_DER, &output));
	file = fopen(filename, "wb");
	assert(file && fwrite(output.data, 1, output.size, file) == output.size);
	assert(!fclose(file));
	gnutls_free(output.data);
	gnutls_pkcs12_bag_deinit(keys);
	gnutls_pkcs12_bag_deinit(certificates);
	gnutls_pkcs12_deinit(archive);
}

int main(int argc, char **argv)
{
	const char *names[] = { "critical extension", "P-521", "RSA-PSS", "RC4 PKCS12" };
	const char *certificates[] = { "rsa-critical-extension.pem", "ec-p521-cert.pem", "rsa-pss-cert.pem", "legacy-RC4-128.p12" };
	const char *keys[] = { "rsa-pkcs8.pem", "ec-p521.pem", "rsa-pss.pem", NULL };
	char certificate[PATH_MAX], key[PATH_MAX], ca[PATH_MAX], expected_file[PATH_MAX], archive[PATH_MAX];
	gnutls_x509_privkey_t renewal_key;
	gnutls_x509_crt_t older, renewed, root;
	gnutls_datum_t key_data;
	assert(argc == 6);
	signal(SIGPIPE, SIG_IGN);
	alarm(30);
	assert(!openconnect_init_ssl());
	path(ca, argv[1], "root.pem");
	for (unsigned i = 0; i < sizeof(names) / sizeof(names[0]); i++) {
		gnutls_x509_crt_t expected;
		path(certificate, argv[1], certificates[i]);
		if (keys[i]) path(key, argv[1], keys[i]);
		path(expected_file, argv[1], i == 3 ? "rsa-cert.pem" : certificates[i]);
		expected = read_certificate(expected_file);
		for (int tls13 = 0; tls13 <= 1; tls13++)
			check(names[i], argv[2], argv[3], argv[4], ca, certificate,
				keys[i] ? key : NULL, i == 3 ? PASSWORD : NULL, expected, i == 0, tls13);
		gnutls_x509_crt_deinit(expected);
	}
	path(key, argv[1], "rsa-pkcs8.pem");
	key_data = read_file(key);
	assert(!gnutls_x509_privkey_init(&renewal_key));
	assert(!gnutls_x509_privkey_import2(renewal_key, &key_data, GNUTLS_X509_FMT_PEM, NULL, 0));
	gnutls_memset(key_data.data, 0, key_data.size);
	free(key_data.data);
	path(expected_file, argv[1], "rsa-cert.pem");
	older = read_certificate(expected_file);
	root = read_certificate(ca);
	renewed = renewed_certificate(renewal_key);
	for (int reverse = 0; reverse <= 1; reverse++) {
		path(archive, argv[5], reverse ? "renewed-reversed.p12" : "renewed-forward.p12");
		renewal_archive(archive, renewal_key, older, renewed, root, reverse);
		for (int tls13 = 0; tls13 <= 1; tls13++)
			check(reverse ? "renewal reversed" : "renewal forward", argv[2], argv[3], argv[4],
				ca, archive, NULL, PASSWORD, reverse ? older : renewed, 0, tls13);
	}
	gnutls_x509_crt_deinit(renewed);
	gnutls_x509_crt_deinit(older);
	gnutls_x509_crt_deinit(root);
	gnutls_x509_privkey_deinit(renewal_key);
	return 0;
}
