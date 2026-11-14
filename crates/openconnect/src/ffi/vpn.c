#include "vpn.h"
#include <errno.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#ifndef _WIN32
#include <unistd.h>
#endif

struct vpn_context {
	struct openconnect_info *vpninfo;
	vpn_command_handle command_fd;
	const vpn_options *options;
	vpn_connected_callback on_connected;
};

static int resolve_gateway(void *private_data, const char *node, const char *service,
			   const struct addrinfo *hints, struct addrinfo **result)
{
	struct vpn_context *context = private_data;
	return vpn_resolve_gateway(node, service, hints, result,
		context->options->gateway_ip_preference);
}

static void observe_configuration(struct vpn_context *context)
{
	const struct oc_ip_info *info = NULL;
	if (openconnect_get_ip_info(context->vpninfo, &info, NULL, NULL))
		info = NULL;
	vpn_tunnel_configuration(context->options->user_data,
		context->options->script_tun ? NULL : openconnect_get_ifname(context->vpninfo), info);
}

static void observe_reconnected(void *private_data)
{
	struct vpn_context *context = private_data;
	observe_configuration(context);
	vpn_tunnel_event(context->options->user_data, VPN_TUNNEL_RECONNECTED);
}

static void observe_tunnel(void *private_data, enum openconnect_tunnel_event event)
{
	struct vpn_context *context = private_data;
	vpn_tunnel_event(context->options->user_data, event);
}

static void observe_statistics(void *private_data, const struct oc_stats *stats)
{
	struct vpn_context *context = private_data;
	vpn_tunnel_statistics(context->options->user_data, stats);
}

/* Validate the peer certificate */
static int validate_peer_cert(void *private_data,
			      const char *reason)
{
	struct vpn_context *context = private_data;
	if (context->options->servercert)
		return openconnect_check_peer_cert_hash(context->vpninfo,
			context->options->servercert);
	VPN_INFO("Accepting the server certificate though %s", reason);
	return 0;
}

/* Print progress messages */
static void print_progress(void *_vpninfo, int level,
			   const char *format, ...)
{
	(void)_vpninfo;
	va_list args;
	va_start(args, format);
	char *message = format_message(format, args);
	va_end(args);

	if (message == NULL) {
		VPN_ERROR("Failed to format log message");
	} else {
		vpn_log(level, message);
		free(message);
	}
}

static void setup_tun_handler(void *private_data)
{
	struct vpn_context *context = private_data;
	struct openconnect_info *_vpninfo = context->vpninfo;
	const vpn_options *options = context->options;
	int ret;
	if (options->script_tun) {
		const struct openconnect_script_tun_launch *launch = options->script_tun_launch;
#ifdef _WIN32
		if (options->script_tun_pipe != -1) {
			ret = openconnect_setup_tun_pipe(_vpninfo, options->script_tun_pipe);
		} else
#endif
		if (launch) {
#ifndef _WIN32
			ret = openconnect_setup_tun_script_exec(_vpninfo, launch);
			vpn_script_started(options->user_data);
#else
			ret = -EOPNOTSUPP;
#endif
		} else {
			ret = openconnect_setup_tun_script(_vpninfo, options->script);
		}
	} else {
		ret = openconnect_setup_tun_device(_vpninfo, options->script,
						   options->interface_name);
	}

	if (!ret) {
		observe_configuration(context);
		vpn_tunnel_event(options->user_data, VPN_TUNNEL_ESTABLISHED);
		vpn_session_info session_info = {
			.auth_expiration = (long)openconnect_get_auth_expiration(_vpninfo),
			.lifetime_secs = openconnect_get_gp_session_lifetime(_vpninfo),
			.user_expires = (long)openconnect_get_gp_user_expires(_vpninfo),
			.lifetime_warning_prior =
			    openconnect_get_gp_lifetime_notify_prior(_vpninfo),
			.lifetime_warning_message =
			    openconnect_get_gp_lifetime_notify_message(_vpninfo),
			.nlb_enabled = openconnect_get_gp_nlb_enabled(_vpninfo),
			.nlb_connected_gw_ip =
			    openconnect_get_gp_nlb_connected_gw_ip(_vpninfo),
		};
		context->on_connected(context->command_fd, &session_info, options->user_data);
	}
}

/* Initialize VPN connection */
int vpn_connect(const vpn_options *options, vpn_connected_callback callback)
{
	struct openconnect_info *vpninfo;
	char hostname[256];
	const char *effective_local_hostname = NULL;

	struct vpn_context context = { .options = options, .on_connected = callback };
	int result = 1;

	VPN_INFO("USER_AGENT: %s", options->user_agent);
	VPN_INFO("OS: %s", options->os);
	VPN_INFO("CLIENT_VERSION: %s", options->client_version);
	VPN_INFO("HOST_ID: %s", options->host_id ? options->host_id : "(not set)");
	const char *configuration = options->script;
	if (options->configure_network)
		configuration = "callback";
	else if (options->script_tun_launch)
		configuration = options->script_tun_launch->program;
	VPN_INFO("Network configuration: %s", configuration);
	VPN_INFO("SCRIPT_TUN: %d", options->script_tun);
	VPN_INFO("CSD_USER: %d", options->hip_script.uid);
	VPN_INFO("CSD_WRAPPER: %s", options->hip_script.path);
	VPN_INFO("RECONNECT_TIMEOUT: %d", options->reconnect_timeout);
	VPN_INFO("MTU: %d", options->mtu);
	VPN_INFO("DISABLE_IPV6: %d", options->disable_ipv6);
	VPN_INFO("GATEWAY_IP_PREFERENCE: %d", options->gateway_ip_preference);
	VPN_INFO("NO_DTLS: %d", options->no_dtls);
	VPN_INFO("DPD_INTERVAL: %d", options->dpd_interval);
	VPN_INFO("NO_XMLPOST: %d", options->no_xmlpost);

	vpninfo =
	    openconnect_vpninfo_new(options->user_agent, validate_peer_cert,
				    NULL, NULL, print_progress, &context);

	if (!vpninfo) {
		VPN_ERROR("openconnect_vpninfo_new failed");
		vpn_tunnel_event(options->user_data, VPN_TUNNEL_ENDED);
		return 1;
	}

	context.vpninfo = vpninfo;
	/* A supplied pin is authoritative even when the system would trust a CA. */
	if (options->servercert)
		openconnect_set_system_trust(vpninfo, 0);
	openconnect_set_tunnel_handler(vpninfo, observe_tunnel);
	openconnect_set_reconnected_handler(vpninfo, observe_reconnected);
	openconnect_set_stats_handler(vpninfo, observe_statistics);
	openconnect_set_gp_hip_report_callback(vpninfo, options->user_data,
					options->on_hip_report_submitted);
	/* Windows command pipes are Winsock sockets. Initialize before creating them. */
	openconnect_init_ssl();
	context.command_fd = openconnect_setup_cmd_pipe(vpninfo);
	if (context.command_fd == (vpn_command_handle)-1) {
		VPN_ERROR("openconnect_setup_cmd_pipe failed");
		goto cleanup;
	}
	/* An earlier disconnect remains latched on this attempt. */
	if (vpn_attach_command_pipe(options->user_data, context.command_fd)) {
		VPN_INFO("VPN attempt canceled before startup");
		result = -EINTR;
		goto cleanup;
	}

	openconnect_set_loglevel(vpninfo, PRG_TRACE);
	openconnect_set_protocol(vpninfo, "gp");
	openconnect_parse_url(vpninfo, options->server);
	if (options->gateway_ip_preference)
		openconnect_override_getaddrinfo(vpninfo, resolve_gateway);
	openconnect_set_cookie(vpninfo, options->cookie);
	openconnect_set_useragent(vpninfo, options->user_agent);

	if (options->os) {
		openconnect_set_reported_os(vpninfo, options->os);
	}

	if (options->os_version) {
		openconnect_set_gp_os_version(vpninfo, options->os_version);
	}

	if (options->client_version) {
		openconnect_set_gp_app_version(vpninfo,
					       options->client_version);
	}

	if (options->host_id) {
		openconnect_set_gp_host_id(vpninfo, options->host_id);
	}

	effective_local_hostname = options->local_hostname;
	if (!effective_local_hostname && !gethostname(hostname, sizeof(hostname))) {
		hostname[sizeof(hostname) - 1] = 0;
		effective_local_hostname = hostname;
	}
	if (effective_local_hostname) {
		openconnect_set_localname(vpninfo, effective_local_hostname);
	}

	VPN_INFO("LOCAL_HOSTNAME: %s",
	     effective_local_hostname ? effective_local_hostname : "(not set)");

	if (options->certificate) {
		VPN_INFO("Setting client certificate: %s", options->certificate);
		openconnect_set_client_cert(vpninfo, options->certificate,
					    options->sslkey);
	}

	if (options->key_password) {
		openconnect_set_key_password(vpninfo, options->key_password);
	}

	if (options->no_xmlpost) {
		openconnect_set_xmlpost(vpninfo, 0);
	}

	openconnect_set_gp_hip_generator(vpninfo, options->user_data, options->generate_hip);
	if (options->hip_script.path) {
		result = openconnect_set_gp_hip_script(vpninfo, options->hip_script.path,
			options->hip_script.uid_present, options->hip_script.uid,
			options->hip_script.validation_data, options->hip_script.validate,
			options->hip_script.environment, options->hip_script.cwd);
		if (result)
			goto cleanup;
	}

	if (options->mtu > 0) {
		int mtu = options->mtu < 576 ? 576 : options->mtu;
		openconnect_set_reqmtu(vpninfo, mtu);
	}

	if (options->disable_ipv6) {
		openconnect_disable_ipv6(vpninfo);
	}

	if (options->dpd_interval > 0) {
		openconnect_set_dpd(vpninfo, options->dpd_interval);
	}
	// Essential step
	vpn_tunnel_event(options->user_data, OC_TUNNEL_NEGOTIATING);
	if (openconnect_make_cstp_connection(vpninfo) != 0) {
		VPN_ERROR("openconnect_make_cstp_connection failed");
		goto cleanup;
	}

	if (options->no_dtls || openconnect_setup_dtls(vpninfo, 60) != 0) {
		openconnect_disable_dtls(vpninfo);
	}
	// Essential step
	openconnect_set_configure_tun_handler(vpninfo, options->user_data,
					     options->configure_network);
	openconnect_set_setup_tun_handler(vpninfo, setup_tun_handler);

	while (1) {
		int ret = openconnect_mainloop(vpninfo,
					       options->reconnect_timeout, 10);

		if (ret) {
			VPN_INFO("openconnect_mainloop returned %d, exiting", ret);
			result = ret;
			goto cleanup;
		}

		VPN_INFO("openconnect_mainloop returned 0, reconnecting");
	}
cleanup:
	vpn_detach_command_pipe(options->user_data);
	openconnect_set_tunnel_handler(vpninfo, NULL);
	openconnect_vpninfo_free(vpninfo);
	vpn_tunnel_event(options->user_data, VPN_TUNNEL_ENDED);
	return result;
}

/* Called with the attempt cancellation mutex held, never after detach/free. */
static int write_command(vpn_command_handle fd, char command)
{
#ifdef _WIN32
	int written = send(fd, &command, 1, 0);
	return written == 1 ? 0 : (written == SOCKET_ERROR ? WSAGetLastError() : EIO);
#else
	ssize_t written;
	do {
		written = write(fd, &command, 1);
	} while (written < 0 && errno == EINTR);
	return written == 1 ? 0 : (written < 0 ? errno : EIO);
#endif
}

int vpn_write_cancel(vpn_command_handle fd)
{
	return write_command(fd, OC_CMD_CANCEL);
}

int vpn_write_statistics(vpn_command_handle fd)
{
	return write_command(fd, OC_CMD_STATS);
}

int vpn_collect_hip_report(const vpn_hip_script *script,
	const struct openconnect_gp_hip_request *request,
	const struct openconnect_gp_hip_control *control, openconnect_gp_hip_generate_fn generate, void *data,
	char *output, size_t capacity, size_t *written)
{
	struct openconnect_info *vpninfo = openconnect_vpninfo_new("HIP preview", NULL, NULL, NULL, print_progress, NULL);
	int result;
	if (!vpninfo)
		return -ENOMEM;
	openconnect_set_gp_hip_generator(vpninfo, data, generate);
	result = openconnect_set_gp_hip_script(vpninfo, script->path, script->uid_present,
		script->uid, script->validation_data, script->validate, script->environment, script->cwd);
	if (!result) result = openconnect_collect_gp_hip_report(vpninfo, request, control, output, capacity, written);
	openconnect_vpninfo_free(vpninfo);
	return result;
}
