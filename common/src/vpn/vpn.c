#include <stdio.h>
#include <openconnect.h>
#include <stdlib.h>
#include <stdarg.h>
#include <time.h>
#include <sys/utsname.h>
#include <unistd.h>

#include "vpn.h"

void *g_user_data;
on_connected_cb g_on_connected_cb;

static int g_cmd_pipe_fd;
const char *g_vpnc_script;

/* Validate the peer certificate */
static int validate_peer_cert(__attribute__((unused)) void *_vpninfo, const char *reason)
{
    printf("Validating peer cert: %s\n", reason);
    return 0;
}

/* Print progress messages */
static void print_progress(__attribute__((unused)) void *_vpninfo, int level, const char *fmt, ...)
{
    FILE *outf = level ? stdout : stderr;
    va_list args;

    char ts[64];
    time_t t = time(NULL);
    struct tm *tm = localtime(&t);

    strftime(ts, 64, "[%Y-%m-%d %H:%M:%S] ", tm);
    fprintf(outf, "%s", ts);

    va_start(args, fmt);
    vfprintf(outf, fmt, args);
    va_end(args);
    fflush(outf);
}

static void setup_tun_handler(void *_vpninfo)
{
    openconnect_setup_tun_device(_vpninfo, g_vpnc_script, NULL);

    if (g_on_connected_cb)
    {
        g_on_connected_cb(g_cmd_pipe_fd, g_user_data);
    }
}

/* Initialize VPN connection */
int start(const Options *options, on_connected_cb cb)
{
    struct openconnect_info *vpninfo;
    struct utsname utsbuf;

    vpninfo = openconnect_vpninfo_new("PAN GlobalProtect", validate_peer_cert, NULL, NULL, print_progress, NULL);

    if (!vpninfo)
    {
        printf("openconnect_vpninfo_new failed\n");
        return 1;
    }

    openconnect_set_loglevel(vpninfo, 1);
    openconnect_init_ssl();
    openconnect_set_protocol(vpninfo, "gp");
    openconnect_set_hostname(vpninfo, options->server);
    openconnect_set_cookie(vpninfo, options->cookie);

    g_cmd_pipe_fd = openconnect_setup_cmd_pipe(vpninfo);
    if (g_cmd_pipe_fd < 0)
    {
        printf("openconnect_setup_cmd_pipe failed\n");
        return 1;
    }

    if (!uname(&utsbuf))
    {
        openconnect_set_localname(vpninfo, utsbuf.nodename);
    }

    // Essential step
    if (openconnect_make_cstp_connection(vpninfo) != 0)
    {
        printf("openconnect_make_cstp_connection failed\n");
        return 1;
    }

    if (openconnect_setup_dtls(vpninfo, 60) != 0)
    {
        openconnect_disable_dtls(vpninfo);
    }

    // Essential step
    // openconnect_setup_tun_device(vpninfo, options->script, NULL);
    g_user_data = options->user_data;
    g_on_connected_cb = cb;
    g_vpnc_script = options->script;
    openconnect_set_setup_tun_handler(vpninfo, setup_tun_handler);

    while (1)
    {
        int ret = openconnect_mainloop(vpninfo, 300, 10);
        printf("openconnect_mainloop returned %d\n", ret);

        if (ret)
        {
            openconnect_vpninfo_free(vpninfo);
            return ret;
        }

        printf("openconnect_mainloop returned\n");
    }
}

/* Stop the VPN connection */
void stop()
{
    char cmd = OC_CMD_CANCEL;
    if (write(g_cmd_pipe_fd, &cmd, 1) < 0)
    {
        printf("Stopping VPN failed\n");
    }
}
