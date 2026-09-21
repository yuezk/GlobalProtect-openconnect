#include <openconnect.h>
#include <errno.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/utsname.h>
#include <unistd.h>

#include "vpn.h"

struct vpn_context {
	struct openconnect_info *vpninfo;
	int command_fd;
	const vpn_options *options;
	vpn_connected_callback on_connected;
};

/* Validate the peer certificate */
static int validate_peer_cert(__attribute__((unused)) void *_vpninfo,
			      const char *reason)
{
	INFO("Accepting the server certificate though %s", reason);
	return 0;
}

/* Print progress messages */
static void print_progress(__attribute__((unused)) void *_vpninfo, int level,
			   const char *format, ...)
{
	va_list args;
	va_start(args, format);
	char *message = format_message(format, args);
	va_end(args);

	if (message == NULL) {
		ERROR("Failed to format log message");
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
		ret = openconnect_setup_tun_script(_vpninfo, options->script);
	} else {
		ret = openconnect_setup_tun_device(_vpninfo, options->script,
						   options->interface);
	}

	if (!ret) {
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
	struct utsname utsbuf;
	const char *effective_local_hostname = NULL;

	struct vpn_context context = { .options = options, .on_connected = callback };
	int result = 1;

	INFO("USER_AGENT: %s", options->user_agent);
	INFO("OS: %s", options->os);
	INFO("CLIENT_VERSION: %s", options->client_version);
	INFO("HOST_ID: %s", options->host_id ? options->host_id : "(not set)");
	INFO("VPNC_SCRIPT: %s", options->script);
	INFO("SCRIPT_TUN: %d", options->script_tun);
	INFO("CSD_USER: %d", options->csd_uid);
	INFO("CSD_WRAPPER: %s", options->csd_wrapper);
	INFO("RECONNECT_TIMEOUT: %d", options->reconnect_timeout);
	INFO("MTU: %d", options->mtu);
	INFO("DISABLE_IPV6: %d", options->disable_ipv6);
	INFO("NO_DTLS: %d", options->no_dtls);
	INFO("DPD_INTERVAL: %d", options->dpd_interval);
	INFO("NO_XMLPOST: %d", options->no_xmlpost);

	vpninfo =
	    openconnect_vpninfo_new(options->user_agent, validate_peer_cert,
				    NULL, NULL, print_progress, &context);

	if (!vpninfo) {
		ERROR("openconnect_vpninfo_new failed");
		return 1;
	}

	context.vpninfo = vpninfo;
	context.command_fd = openconnect_setup_cmd_pipe(vpninfo);
	if (context.command_fd < 0) {
		ERROR("openconnect_setup_cmd_pipe failed");
		goto cleanup;
	}
	/* An earlier disconnect remains latched on this attempt. */
	if (vpn_attach_command_pipe(options->user_data, context.command_fd)) {
		INFO("VPN attempt canceled before startup");
		result = -EINTR;
		goto cleanup;
	}

	openconnect_set_loglevel(vpninfo, PRG_TRACE);
	openconnect_init_ssl();
	openconnect_set_protocol(vpninfo, "gp");
	openconnect_parse_url(vpninfo, options->server);
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
	if (!effective_local_hostname && !uname(&utsbuf)) {
		effective_local_hostname = utsbuf.nodename;
	}
	if (effective_local_hostname) {
		openconnect_set_localname(vpninfo, effective_local_hostname);
	}

	INFO("LOCAL_HOSTNAME: %s",
	     effective_local_hostname ? effective_local_hostname : "(not set)");

	if (options->certificate) {
		INFO("Setting client certificate: %s", options->certificate);
		openconnect_set_client_cert(vpninfo, options->certificate,
					    options->sslkey);
	}

	if (options->key_password) {
		openconnect_set_key_password(vpninfo, options->key_password);
	}

	if (options->no_xmlpost) {
		openconnect_set_xmlpost(vpninfo, 0);
	}

	if (options->csd_wrapper) {
		openconnect_setup_csd(vpninfo, options->csd_uid, 1,
				      options->csd_wrapper);
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
	if (openconnect_make_cstp_connection(vpninfo) != 0) {
		ERROR("openconnect_make_cstp_connection failed");
		goto cleanup;
	}

	if (options->no_dtls || openconnect_setup_dtls(vpninfo, 60) != 0) {
		openconnect_disable_dtls(vpninfo);
	}
	// Essential step
	openconnect_set_setup_tun_handler(vpninfo, setup_tun_handler);

	while (1) {
		int ret = openconnect_mainloop(vpninfo,
					       options->reconnect_timeout, 10);

		if (ret) {
			INFO("openconnect_mainloop returned %d, exiting", ret);
			result = ret;
			goto cleanup;
		}

		INFO("openconnect_mainloop returned 0, reconnecting");
	}
cleanup:
	vpn_detach_command_pipe(options->user_data);
	openconnect_vpninfo_free(vpninfo);
	return result;
}

/* Called with the attempt cancellation mutex held, never after detach/free. */
int vpn_write_cancel(int fd)
{
	char command = OC_CMD_CANCEL;
	ssize_t written;
	do {
		written = write(fd, &command, 1);
	} while (written < 0 && errno == EINTR);
	return written == 1 ? 0 : (written < 0 ? errno : EIO);
}
