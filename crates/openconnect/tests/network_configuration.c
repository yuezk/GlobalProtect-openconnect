/* No privileged networking: exercise the actual configuration dispatcher. */
#include <config.h>
#include "openconnect-internal.h"
#include <assert.h>

struct state {
	pid_t parent;
	const char *reason;
	int calls;
	int result;
	const char *failure_reason;
	const char **expected_reasons;
	int expected_count;
	int tcp_connections;
	int reconnection_notifications;
	int reconnect_started;
};

static int configure(void *data, const char *reason, const struct oc_vpn_option *env)
{
	struct state *state = data;
	const struct oc_vpn_option *option;
	int found = 0;
	assert(getpid() == state->parent);
	if (state->expected_reasons) {
		assert(state->calls < state->expected_count);
		assert(!strcmp(reason, state->expected_reasons[state->calls]));
	} else {
		assert(!strcmp(reason, state->reason));
	}
	for (option = env; option; option = option->next) {
		if (!strcmp(option->option, "GP_TEST_CONFIG")) {
			assert(!strcmp(option->value, "child value"));
			found = 1;
		}
	}
	assert(found);
	assert(!strcmp(getenv("GP_TEST_CONFIG"), "parent value"));
	state->calls++;
	return !state->failure_reason || !strcmp(reason, state->failure_reason) ? state->result : 0;
}

static void progress(void *data, int level, const char *format, ...) {}


/* The production library.c is compiled with os_setup_tun redirected here.
 * All setup, configuration dispatch, attachment, and cleanup code stays real. */
static int device_fd = -1;
static int device_creations;
intptr_t test_os_setup_tun(struct openconnect_info *vpninfo)
{
	device_creations++;
	assert(device_fd >= 0);
	free(vpninfo->ifname);
	vpninfo->ifname = strdup("gp-test-tun");
	assert(vpninfo->ifname);
	return device_fd;
}

static struct openconnect_info *new_vpn(struct state *state)
{
	struct openconnect_info *vpninfo = openconnect_vpninfo_new("network callback test", NULL, NULL, NULL, progress, state);
	assert(vpninfo);
	script_setenv(vpninfo, "GP_TEST_CONFIG", "child value", 0, 0);
	openconnect_set_configure_tun_handler(vpninfo, state, configure);
	return vpninfo;
}

static void connect_failure_rolls_back(void)
{
	const char *expected[] = { "pre-init", "connect", "disconnect" };
	struct state state = {
		.parent = getpid(), .result = -EPERM, .failure_reason = "connect",
		.expected_reasons = expected, .expected_count = 3,
	};
	struct openconnect_info *vpninfo = new_vpn(&state);
	int fds[2];
	assert(!pipe(fds));
	device_fd = fds[0];
	assert(openconnect_setup_tun_device(vpninfo, NULL, NULL) == -EPERM);
	assert(device_creations == 1);
	assert(state.calls == 3);
	assert(vpninfo->tun_fd == -1);
	assert(fcntl(fds[0], F_GETFD) == -1 && errno == EBADF);
	close(fds[1]);
	openconnect_vpninfo_free(vpninfo);
	/* Freeing an already rolled-back connection must not disconnect again. */
	assert(state.calls == 3);
	device_fd = -1;
}

static int tcp_connect(struct openconnect_info *vpninfo)
{
	struct state *state = vpninfo->cbdata;
	state->tcp_connections++;
	return 0;
}

static void reconnected(void *data)
{
	struct state *state = data;
	state->reconnection_notifications++;
}

static void tunnel_event(void *data, enum openconnect_tunnel_event event)
{
	struct state *state = data;
	assert(event == OC_TUNNEL_RECONNECTING);
	state->reconnect_started++;
	/* Start observation precedes configuration callbacks and new TCP work. */
	assert(state->calls == 0);
}

static void reconnect_refusal_propagates(void)
{
	const char *expected[] = { "attempt-reconnect", "reconnect" };
	struct state state = {
		.parent = getpid(), .result = -EPERM, .failure_reason = "attempt-reconnect",
		.expected_reasons = expected, .expected_count = 1,
	};
	struct openconnect_info *vpninfo = new_vpn(&state);
	const struct vpn_proto *original = vpninfo->proto;
	struct vpn_proto protocol = *original;
	int fds[2];
	assert(!pipe(fds));
	vpninfo->tun_fd = fds[0];
	protocol.tcp_connect = tcp_connect;
	protocol.proto = PROTO_GPST;
	vpninfo->proto = &protocol;
	openconnect_set_reconnected_handler(vpninfo, reconnected);
	openconnect_set_tunnel_handler(vpninfo, tunnel_event);

	/* Attempt refusal prevents even the protocol's reconnect request. */
	assert(ssl_reconnect(vpninfo) == -EPERM);
	assert(state.calls == 1 && state.tcp_connections == 0);
	assert(state.reconnection_notifications == 0);

	/* Post-reconnect refusal prevents the successful reconnection observer. */
	state.calls = 0;
	state.expected_count = 2;
	state.failure_reason = "reconnect";
	state.result = -EACCES;
	assert(ssl_reconnect(vpninfo) == -EACCES);
	assert(state.calls == 2 && state.tcp_connections == 1);
	assert(state.reconnection_notifications == 0);

	state.calls = 0;
	state.result = 0;
	assert(!ssl_reconnect(vpninfo));
	assert(state.calls == 2 && state.tcp_connections == 2);
	assert(state.reconnection_notifications == 1);
	assert(state.reconnect_started == 3);

	state.expected_reasons = NULL;
	state.reason = "disconnect";
	os_shutdown_tun(vpninfo);
	assert(fcntl(fds[0], F_GETFD) == -1 && errno == EBADF);
	close(fds[1]);
	vpninfo->proto = original;
	openconnect_vpninfo_free(vpninfo);
}

int main(void)
{
	const char *reasons[] = { "pre-init", "connect", "attempt-reconnect", "reconnect", "disconnect" };
	struct state state = { .parent = getpid() };
	struct openconnect_info *vpninfo = new_vpn(&state);
	int fds[2], i;
	assert(vpninfo);
	assert(!setenv("GP_TEST_CONFIG", "parent value", 1));
	script_setenv(vpninfo, "GP_TEST_CONFIG", "child value", 0, 0);
	openconnect_set_configure_tun_handler(vpninfo, &state, configure);
	/* A callback works without an external script and executes before fork. */
	assert(!vpninfo->vpnc_script);
	for (i = 0; i < 5; i++) {
		state.reason = reasons[i];
		assert(!script_config_tun(vpninfo, reasons[i]));
	}
	assert(state.calls == 5);
	state.result = -EPERM;
	state.reason = "pre-init";
	/* Refusal propagates before attempting to create an OS tunnel device. */
	assert(openconnect_setup_tun_device(vpninfo, NULL, NULL) == -EPERM);
	assert(vpninfo->tun_fd == -1);
	assert(device_creations == 0);
	state.result = 0;
	state.reason = "disconnect";
	/* The callback path owns the created descriptor even with no script path. */
	assert(!pipe(fds));
	vpninfo->tun_fd = fds[0];
	os_shutdown_tun(vpninfo);
	assert(vpninfo->tun_fd == -1);
	assert(fcntl(fds[0], F_GETFD) == -1 && errno == EBADF);
	close(fds[1]);
	assert(state.calls == 7);
	openconnect_set_configure_tun_handler(vpninfo, NULL, NULL);
	assert(!script_config_tun(vpninfo, "connect"));
	assert(state.calls == 7);
	openconnect_vpninfo_free(vpninfo);
	connect_failure_rolls_back();
	reconnect_refusal_propagates();
	return 0;
}
