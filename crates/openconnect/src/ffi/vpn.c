#include <stdio.h>
#include <stdlib.h>
#include <stdarg.h>
#include <unistd.h>
#include <sys/utsname.h>
#include <openconnect.h>

#include "vpn.h"

void *g_user_data;

static int g_cmd_pipe_fd;
static const char *g_vpnc_script;
static vpn_connected_callback on_vpn_connected;

/* Validate the peer certificate */
static int validate_peer_cert(__attribute__((unused)) void *_vpninfo, const char *reason)
{
    INFO("Accepting the server certificate though %s", reason);
    return 0;
}

/* Print progress messages */
static void print_progress(__attribute__((unused)) void *_vpninfo, int level, const char *format, ...)
{
    va_list args;
    va_start(args, format);
    char *message = format_message(format, args);
    va_end(args);

    if (message == NULL) {
        ERROR("Failed to format log message");
    } else {
        LOG(level, message);
        free(message);
    }
}

static void setup_tun_handler(void *_vpninfo)
{
    int ret = openconnect_setup_tun_device(_vpninfo, g_vpnc_script, NULL);
    if (!ret) {
        on_vpn_connected(g_cmd_pipe_fd, g_user_data);
    }
}

/* Initialize VPN connection */
int vpn_connect(const vpn_options *options, vpn_connected_callback callback)
{
    INFO("openconnect version: %s", openconnect_get_version());
    struct openconnect_info *vpninfo;
    struct utsname utsbuf;

    g_user_data = options->user_data;
    g_vpnc_script = options->script;
    on_vpn_connected = callback;

    INFO("User agent: %s", options->user_agent);
    INFO("VPNC script: %s", options->script);
    INFO("OS: %s", options->os);
    INFO("CSD_USER: %d", options->csd_uid);
    INFO("CSD_WRAPPER: %s", options->csd_wrapper);
    INFO("RECONNECT_TIMEOUT: %d", options->reconnect_timeout);
    INFO("MTU: %d", options->mtu);
    INFO("DISABLE_IPV6: %d", options->disable_ipv6);
    INFO("NO_DTLS: %d", options->no_dtls);

    vpninfo = openconnect_vpninfo_new(options->user_agent, validate_peer_cert, NULL, NULL, print_progress, NULL);

    if (!vpninfo) {
        ERROR("openconnect_vpninfo_new failed");
        return 1;
    }

    openconnect_set_loglevel(vpninfo, PRG_TRACE);
    openconnect_init_ssl();
    openconnect_set_protocol(vpninfo, "gp");
    openconnect_set_hostname(vpninfo, options->server);
    openconnect_set_cookie(vpninfo, options->cookie);

    if (options->os) {
        openconnect_set_reported_os(vpninfo, options->os);
    }

    if (options->certificate) {
        INFO("Setting client certificate: %s", options->certificate);
        openconnect_set_client_cert(vpninfo, options->certificate, options->sslkey);
    }

    if (options->key_password) {
        openconnect_set_key_password(vpninfo, options->key_password);
    }

    if (options->csd_wrapper) {
        openconnect_setup_csd(vpninfo, options->csd_uid, 1, options->csd_wrapper);
    }

    if (options->mtu > 0) {
        int mtu = options->mtu < 576 ? 576 : options->mtu;
        openconnect_set_reqmtu(vpninfo, mtu);
    }

    if (options->disable_ipv6) {
        openconnect_disable_ipv6(vpninfo);
    }

    g_cmd_pipe_fd = openconnect_setup_cmd_pipe(vpninfo);
    if (g_cmd_pipe_fd < 0) {
        ERROR("openconnect_setup_cmd_pipe failed");
        return 1;
    }

    if (!uname(&utsbuf)) {
        openconnect_set_localname(vpninfo, utsbuf.nodename);
    }

    // Essential step
    if (openconnect_make_cstp_connection(vpninfo) != 0) {
        ERROR("openconnect_make_cstp_connection failed");
        return 1;
    }

    if (options->no_dtls || openconnect_setup_dtls(vpninfo, 60) != 0) {
        openconnect_disable_dtls(vpninfo);
    }

    // Essential step
    openconnect_set_setup_tun_handler(vpninfo, setup_tun_handler);

    while (1) {
        int ret = openconnect_mainloop(vpninfo, options->reconnect_timeout, 10);

        if (ret) {
            INFO("openconnect_mainloop returned %d, exiting", ret);
            openconnect_vpninfo_free(vpninfo);
            return ret;
        }

        INFO("openconnect_mainloop returned 0, reconnecting");
    }
}

/* Stop the VPN connection */
void vpn_disconnect()
{
    char cmd = OC_CMD_CANCEL;

    INFO("Stopping VPN connection: %d", g_cmd_pipe_fd);

    if (write(g_cmd_pipe_fd, &cmd, 1) < 0) {
        ERROR("Failed to write to command pipe, VPN connection may not be stopped");
    }
}
