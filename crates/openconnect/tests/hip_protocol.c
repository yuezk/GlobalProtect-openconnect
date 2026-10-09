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
	/* Shared golden fixture with gpapi's non-tunnel HIP transport. */
	assert(!strcmp(request->md5, "20f30054f721fd212925866b5778447d"));
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

static int validate_script(void *data, const struct openconnect_gp_hip_control *control)
{
	return control->check(control->data);
}

static void script_registration_lifecycle(struct openconnect_info *vpninfo)
{
	const char *environment[] = { "PATH=/usr/bin:/bin", NULL };
	char *path, **saved_environment, *cwd;
	int validator_data;
	uid_t uid = getuid();
	gid_t gid;

	assert(!openconnect_set_gp_hip_script(vpninfo, "/bin/true", 1, uid,
		&validator_data, validate_script, environment, "/"));
	path = vpninfo->csd_wrapper;
	saved_environment = vpninfo->gp_hip_environment;
	cwd = vpninfo->gp_hip_cwd;
	gid = vpninfo->gid_csd;

	/* A refused update must preserve the entire registered policy. */
	assert(openconnect_set_gp_hip_script(vpninfo, "/bin/false", 1, (uid_t)-1,
		NULL, NULL, NULL, NULL) < 0);
	assert(vpninfo->csd_wrapper == path);
	assert(vpninfo->gp_hip_environment == saved_environment);
	assert(vpninfo->gp_hip_cwd == cwd);
	assert(vpninfo->uid_csd_given == 1 && vpninfo->uid_csd == uid && vpninfo->gid_csd == gid);
	assert(vpninfo->gp_hip_validate == validate_script);
	assert(vpninfo->gp_hip_validate_data == &validator_data);

	/* Re-registration may use pointers borrowed from the existing policy. */
	assert(!openconnect_set_gp_hip_script(vpninfo, path, 0, 0,
		&validator_data, validate_script, (const char *const *)saved_environment, cwd));
	assert(!strcmp(vpninfo->csd_wrapper, "/bin/true"));
	assert(!strcmp(vpninfo->gp_hip_environment[0], environment[0]));
	assert(!strcmp(vpninfo->gp_hip_cwd, "/"));
	assert(!vpninfo->uid_csd_given);

	/* Legacy replacement must not retain another script's approval policy. */
	assert(!openconnect_setup_csd(vpninfo, uid, 1, "/bin/false"));
	assert(!strcmp(vpninfo->csd_wrapper, "/bin/false"));
	assert(vpninfo->uid_csd_given == 2 && vpninfo->uid_csd == uid && vpninfo->gid_csd == gid);
	assert(!vpninfo->gp_hip_validate && !vpninfo->gp_hip_validate_data);
	assert(!vpninfo->gp_hip_environment && !vpninfo->gp_hip_cwd);
	assert(!openconnect_set_gp_hip_script(vpninfo, NULL, 0, 0, NULL, NULL, NULL, NULL));
	assert(!vpninfo->csd_wrapper);
}

static int close_count;
static int count_session_close(struct openconnect_info *vpninfo, const char *reason)
{
	assert(!strcmp(reason, "fixture exit"));
	close_count++;
	return 0;
}

static void native_session_logout(void)
{
	struct openconnect_info *vpninfo = openconnect_vpninfo_new("logout owner test",
		accept_cert, NULL, NULL, progress, NULL);
	const struct vpn_proto *original;
	struct vpn_proto protocol;
	assert(vpninfo);
	assert(!openconnect_set_protocol(vpninfo, "gp"));
	original = vpninfo->proto;
	protocol = *original;
	protocol.vpn_close_session = count_session_close;
	vpninfo->proto = &protocol;
	vpninfo->quit_reason = "fixture exit";
	/* All exit causes use this same mainloop cleanup path. */
	openconnect_mainloop(vpninfo, 0, 0);
	assert(close_count == 1);
	openconnect_mainloop(vpninfo, 0, 0);
	assert(close_count == 2);
	vpninfo->proto = original;
	openconnect_vpninfo_free(vpninfo);
}

int main(int argc, char **argv) {
	struct state state = {0};
	struct openconnect_info *vpninfo;
	assert(argc == 2);
	native_session_logout();
	openconnect_init_ssl();
	vpninfo = openconnect_vpninfo_new("HIP protocol test", accept_cert, NULL, NULL, progress, &state);
	assert(vpninfo);
	script_registration_lifecycle(vpninfo);
	assert(!openconnect_set_protocol(vpninfo, "gp"));
	assert(!openconnect_set_reported_os(vpninfo, "linux"));
	assert(!openconnect_parse_url(vpninfo, argv[1]));
	assert(!openconnect_set_cookie(vpninfo, "user=test&authcookie=test&persistent-cookie=persistent%2Bcookie&portal=test&domain=test&preferred-ip=192.0.2.1&preferred-ipv6=2001%3Adb8%3A%3A1&computer=test"));
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
	assert(!openconnect_set_cookie(vpninfo, "user=test&authcookie=refreshed&persistent-cookie=persistent%2Bcookie&portal=test&domain=test&preferred-ip=192.0.2.2&preferred-ipv6=2001%3Adb8%3A%3A2&computer=test"));
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
