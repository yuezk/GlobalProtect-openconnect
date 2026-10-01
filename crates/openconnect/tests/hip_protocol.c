/* Integration harness: exercise setup and the actual connected periodic branch. */
#include <config.h>
#include "openconnect-internal.h"
#include <assert.h>
#include <stdarg.h>

struct state { int generated, accepted; };
static int accept_cert(void *data, const char *reason) { return 0; }
static void progress(void *data, int level, const char *format, ...) {
	if (level > PRG_INFO) return;
	va_list args; va_start(args, format); vfprintf(stderr, format, args); va_end(args);
}
static int generate(void *data, const struct openconnect_gp_hip_request *request,
	const struct openconnect_gp_hip_control *control, char *output, size_t capacity, size_t *written)
{
	struct state *state = data;
	int length;
	assert(!control->check(control->data));
	assert(!strcmp(request->client_os, "Linux"));
	assert(!strcmp(request->client_version, "6.3.3"));
	assert(!strcmp(request->host_id, "test-host"));
	assert(request->cookie && request->md5 && strlen(request->md5) == 32);
	if (state->generated >= 2) assert(strstr(request->cookie, "authcookie=refreshed"));
	state->generated++;
	length = snprintf(output, capacity, "<hip seq=\"%d\" ip=\"%s\"/>", state->generated, request->client_ip);
	assert(length > 0 && length < capacity);
 *written = length;
	return 0;
}
static void submitted(void *data, const char *report, size_t length) {
	struct state *state = data;
	assert(report && length > 0);
	state->accepted++;
}
static int periodic(struct openconnect_info *vpninfo) {
	int timeout = 1000;
	vpninfo->trojan_interval = 1;
	vpninfo->last_trojan = time(NULL) - 2;
	return gpst_mainloop(vpninfo, &timeout, 0);
}
int main(int argc, char **argv) {
	struct state state = {0};
	struct openconnect_info *vpninfo;
	assert(argc == 2);
	openconnect_init_ssl();
	vpninfo = openconnect_vpninfo_new("HIP protocol test", accept_cert, NULL, NULL, progress, &state);
	assert(vpninfo);
	assert(!openconnect_set_protocol(vpninfo, "gp"));
	assert(!openconnect_set_reported_os(vpninfo, "linux"));
	assert(!openconnect_parse_url(vpninfo, argv[1]));
	assert(!openconnect_set_cookie(vpninfo, "user=test&authcookie=test&portal=test&domain=test&computer=test"));
	assert(!openconnect_set_gp_app_version(vpninfo, "6.3.3"));
	assert(!openconnect_set_gp_host_id(vpninfo, "test-host"));
	openconnect_disable_dtls(vpninfo);
	openconnect_set_gp_hip_generator(vpninfo, &state, generate);
	openconnect_set_gp_hip_report_callback(vpninfo, &state, submitted);
	assert(!openconnect_make_cstp_connection(vpninfo));
	assert(vpninfo->ssl_fd >= 0 && state.generated == 1 && state.accepted == 1);
	assert(!periodic(vpninfo));
	assert(state.generated == 2 && state.accepted == 2);
	/* A real reconnect re-fetches configuration and uses refreshed session inputs. */
	openconnect_close_https(vpninfo, 0);
	assert(!openconnect_set_cookie(vpninfo, "user=test&authcookie=refreshed&portal=test&domain=test&computer=test"));
	assert(!openconnect_make_cstp_connection(vpninfo));
	assert(state.generated == 3 && state.accepted == 3);
	/* Fourth gateway check says no report needed. */
	assert(!periodic(vpninfo));
	assert(state.generated == 3 && state.accepted == 3);
	/* Fifth submission is rejected; observer must not see it. */
	assert(periodic(vpninfo));
	assert(state.generated == 4 && state.accepted == 3);
	openconnect_vpninfo_free(vpninfo);
	return 0;
}
