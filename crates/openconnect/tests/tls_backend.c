/* Exercise the production OpenConnect TLS client against a local GnuTLS peer. */
#include <config.h>
#include "openconnect-internal.h"
#include <gnutls/gnutls.h>
#include <assert.h>
#include <pthread.h>
#include <signal.h>
#include <stdarg.h>

struct peer {
	int listener, legacy, expect_rejection, handshake, renegotiation, require_identity;
	gnutls_certificate_credentials_t credentials;
};
struct policy { int accept, calls, unsafe_renegotiation_denied; };

struct stalled_logout_peer {
	int listener, stop, complete_tls;
	gnutls_certificate_credentials_t credentials;
};

static void *stall_logout(void *data)
{
	struct stalled_logout_peer *peer = data;
	struct timeval timeout = { .tv_sec = 7 };
	fd_set reads;
	gnutls_session_t session = NULL;
	int fd = accept(peer->listener, NULL, NULL);
	assert(fd >= 0);
	if (peer->complete_tls) {
		assert(!gnutls_init(&session, GNUTLS_SERVER));
		assert(!gnutls_set_default_priority(session));
		assert(!gnutls_credentials_set(session, GNUTLS_CRD_CERTIFICATE, peer->credentials));
		gnutls_transport_set_int(session, fd);
		gnutls_handshake_set_timeout(session, 5000);
		assert(!gnutls_handshake(session));
	}
	/* Stall either the TLS handshake or the HTTP logout response. The watchdog
	 * lets the regression fail without leaving either peer blocked forever. */
	FD_ZERO(&reads);
	FD_SET(peer->stop, &reads);
	select(peer->stop + 1, &reads, NULL, NULL, &timeout);
	if (session)
		gnutls_deinit(session);
	close(fd);
	return NULL;
}

static void progress(void *data, int level, const char *format, ...)
{
	va_list args;
	char message[2048];
	struct policy *policy = data;
	if (level > PRG_DEBUG) return;
	va_start(args, format);
	vsnprintf(message, sizeof(message), format, args);
	va_end(args);
	fputs(message, stderr);
	if (strstr(message, gnutls_strerror(GNUTLS_E_UNSAFE_RENEGOTIATION_DENIED)))
		policy->unsafe_renegotiation_denied = 1;
}

static int certificate_policy(void *data, const char *reason)
{
	struct policy *policy = data;
	assert(reason && *reason);
	policy->calls++;
	return policy->accept ? 0 : -EINVAL;
}

static void logout_finishes_when_gateway_stalls(const char *cert, const char *key, int complete_tls)
{
	struct stalled_logout_peer peer;
	struct policy policy = { .accept = 1 };
	struct sockaddr_in address = { .sin_family = AF_INET, .sin_addr.s_addr = htonl(INADDR_LOOPBACK) };
	socklen_t length = sizeof(address);
	struct timespec start, end;
	struct openconnect_info *client;
	pthread_t thread;
	char url[128];
	int stop[2];
	double elapsed;

	assert(!pipe(stop));
	peer.complete_tls = complete_tls;
	assert(!gnutls_certificate_allocate_credentials(&peer.credentials));
	assert(!gnutls_certificate_set_x509_key_file(peer.credentials, cert, key, GNUTLS_X509_FMT_PEM));
	peer.stop = stop[0];
	peer.listener = socket(AF_INET, SOCK_STREAM, 0);
	assert(peer.listener >= 0);
	assert(!bind(peer.listener, (struct sockaddr *)&address, sizeof(address)));
	assert(!getsockname(peer.listener, (struct sockaddr *)&address, &length));
	assert(!listen(peer.listener, 1));
	assert(!pthread_create(&thread, NULL, stall_logout, &peer));
	client = openconnect_vpninfo_new("Logout timeout regression", certificate_policy, NULL, NULL, progress, &policy);
	assert(client);
	snprintf(url, sizeof(url), "https://127.0.0.1:%u/", ntohs(address.sin_port));
	assert(!openconnect_parse_url(client, url));
	assert(!openconnect_set_protocol(client, "gp"));
	assert(!openconnect_set_cookie(client, "user=fixture&authcookie=fixture"));
	/* Enter the same cleanup branch as a consumed OC_CMD_CANCEL. */
	client->quit_reason = "Aborted by caller";
	assert(!clock_gettime(CLOCK_MONOTONIC, &start));
	assert(openconnect_mainloop(client, 0, 0) < 0);
	assert(!client->https_deadline.tv_sec && !client->https_deadline.tv_nsec);
	assert(!clock_gettime(CLOCK_MONOTONIC, &end));
	elapsed = end.tv_sec - start.tv_sec + (end.tv_nsec - start.tv_nsec) / 1e9;
	(void)write(stop[1], "x", 1);
	assert(!pthread_join(thread, NULL));
	close(stop[0]);
	close(stop[1]);
	close(peer.listener);
	gnutls_certificate_free_credentials(peer.credentials);
	openconnect_vpninfo_free(client);
	assert(elapsed < 6.0);
}

static void *serve(void *data)
{
	struct peer *peer = data;
	gnutls_session_t session;
	int fd = accept(peer->listener, NULL, NULL);
	struct timeval timeout = { .tv_sec = 5 };
	assert(fd >= 0);
	assert(!setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout)));
	assert(!setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof(timeout)));
	assert(!gnutls_init(&session, GNUTLS_SERVER));
	assert(!gnutls_priority_set_direct(session, peer->legacy
		? "NORMAL:-VERS-ALL:+VERS-TLS1.2:%DISABLE_SAFE_RENEGOTIATION"
		: "NORMAL:-VERS-ALL:+VERS-TLS1.2", NULL));
	assert(!gnutls_credentials_set(session, GNUTLS_CRD_CERTIFICATE, peer->credentials));
	if (peer->require_identity)
		gnutls_certificate_server_set_request(session, GNUTLS_CERT_REQUIRE);
	gnutls_transport_set_int(session, fd);
	gnutls_handshake_set_timeout(session, 5000);
	peer->handshake = gnutls_handshake(session);
	if (!peer->expect_rejection) {
		unsigned int status, count;
		assert(!peer->handshake);
		if (peer->require_identity) {
			assert(gnutls_certificate_get_peers(session, &count) && count > 0);
			assert(!gnutls_certificate_verify_peers2(session, &status));
			assert(!status);
		}
		assert(gnutls_safe_renegotiation_status(session) == !peer->legacy);
		assert(gnutls_record_send(session, "OK", 2) == 2);
		if (peer->legacy) {
			assert(!gnutls_rehandshake(session));
			peer->renegotiation = gnutls_handshake(session);
		}
	}
	gnutls_deinit(session);
	close(fd);
	return NULL;
}

static void check(const char *cert, const char *key, const char *ca,
	int legacy, int trusted, int accept_certificate, const char *identity, const char *identity_key)
{
	struct peer peer = { .legacy = legacy, .expect_rejection = !trusted && !accept_certificate,
		.require_identity = identity != NULL };
	struct policy policy = { .accept = accept_certificate };
	struct sockaddr_in address = { .sin_family = AF_INET, .sin_addr.s_addr = htonl(INADDR_LOOPBACK) };
	socklen_t address_length = sizeof(address);
	pthread_t thread;
	char url[128], record[16];
	int result;
	struct openconnect_info *client;
	assert(!gnutls_certificate_allocate_credentials(&peer.credentials));
	assert(!gnutls_certificate_set_x509_key_file(peer.credentials, cert, key, GNUTLS_X509_FMT_PEM));
	if (peer.require_identity)
		assert(gnutls_certificate_set_x509_trust_file(peer.credentials, ca, GNUTLS_X509_FMT_PEM) > 0);
	peer.listener = socket(AF_INET, SOCK_STREAM, 0);
	assert(peer.listener >= 0);
	assert(!bind(peer.listener, (struct sockaddr *)&address, sizeof(address)));
	assert(!getsockname(peer.listener, (struct sockaddr *)&address, &address_length));
	assert(!listen(peer.listener, 1));
	assert(!pthread_create(&thread, NULL, serve, &peer));
	client = openconnect_vpninfo_new("TLS backend regression", certificate_policy, NULL, NULL, progress, &policy);
	assert(client);
	/* localhost matches the fixture certificate; no external DNS or connection. */
	snprintf(url, sizeof(url), "https://localhost:%u/", ntohs(address.sin_port));
	assert(!openconnect_parse_url(client, url));
	if (trusted) assert(!openconnect_set_cafile(client, ca));
	if (identity) assert(!openconnect_set_client_cert(client, identity, identity_key));
	result = openconnect_open_https(client);
	if (peer.expect_rejection) {
		assert(result < 0);
		assert(policy.calls == 1);
	} else {
		assert(!result);
		assert(policy.calls == !trusted);
		assert(client->ssl_read(client, record, sizeof(record)) == 2);
		assert(!memcmp(record, "OK", 2));
		if (legacy) {
			assert(client->ssl_read(client, record, sizeof(record)) < 0);
			/* OpenConnect translates backend failures to -EIO. Its progress
			 * callback preserves the specific GnuTLS failure; require that
			 * reason so a timeout/disconnect cannot pass as policy refusal. */
			assert(policy.unsafe_renegotiation_denied);
		}
	}
	openconnect_vpninfo_free(client);
	assert(!pthread_join(thread, NULL));
	if (legacy) {
		assert(peer.renegotiation < 0);
	}
	close(peer.listener);
	gnutls_certificate_free_credentials(peer.credentials);
}

int main(int argc, char **argv)
{
	assert(argc == 6);
	signal(SIGPIPE, SIG_IGN);
	assert(!openconnect_init_ssl());
	logout_finishes_when_gateway_stalls(argv[1], argv[2], 0);
	logout_finishes_when_gateway_stalls(argv[1], argv[2], 1);
#ifdef __APPLE__
	/* Read-only check of the source-built library's native Keychain integration.
	 * Local fixture trust is tested separately, without changing OS trust settings. */
	{
		gnutls_certificate_credentials_t system;
		assert(!gnutls_certificate_allocate_credentials(&system));
		assert(gnutls_certificate_set_x509_system_trust(system) > 0);
		gnutls_certificate_free_credentials(system);
	}
#endif
	check(argv[1], argv[2], argv[3], 0, 1, 0, NULL, NULL);
	check(argv[1], argv[2], argv[3], 0, 0, 0, NULL, NULL);
	check(argv[1], argv[2], argv[3], 0, 0, 1, NULL, NULL);
	check(argv[1], argv[2], argv[3], 1, 1, 0, NULL, NULL);
	check(argv[1], argv[2], argv[3], 0, 1, 0, argv[4], argv[5]);
	puts("OpenConnect: trusted TLS, certificate rejection/acceptance, legacy initial handshake, unsafe renegotiation refusal, mutual TLS passed");
	return 0;
}
