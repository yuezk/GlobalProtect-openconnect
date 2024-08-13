#include <stdio.h>
#include <stdlib.h>
#include <stdarg.h>
#include <openconnect.h>

typedef void (*vpn_connected_callback)(int cmd_pipe_fd, void *user_data);

typedef struct vpn_options
{
    void *user_data;
    const char *server;
    const char *cookie;
    const char *user_agent;

    const char *script;
    const char *os;
    const char *certificate;
    const char *sslkey;
    const char *key_password;
    const char *servercert;

    const uid_t csd_uid;
    const char *csd_wrapper;

    const int reconnect_timeout;
    const int mtu;
    const int disable_ipv6;
    const int no_dtls;
} vpn_options;

int vpn_connect(const vpn_options *options, vpn_connected_callback callback);
void vpn_disconnect();

extern void vpn_log(int level, const char *msg);

static char *format_message(const char *format, va_list args)
{
    va_list args_copy;
    va_copy(args_copy, args);
    int len = vsnprintf(NULL, 0, format, args_copy);
    va_end(args_copy);

    char *buffer = (char*)malloc(len + 1);
    if (buffer == NULL)
    {
        return NULL;
    }

    vsnprintf(buffer, len + 1, format, args);
    return buffer;
}

static void _log(int level, ...)
{
    va_list args;
    va_start(args, level);

    char *format = va_arg(args, char *);
    char *message = format_message(format, args);

    va_end(args);

    if (message == NULL)
    {
        vpn_log(PRG_ERR, "Failed to format log message");
    }
    else
    {
        vpn_log(level, message);
        free(message);
    }
}

#define LOG(level, ...) _log(level, __VA_ARGS__)
#define ERROR(...) LOG(PRG_ERR, __VA_ARGS__)
#define INFO(...) LOG(PRG_INFO, __VA_ARGS__)
#define DEBUG(...) LOG(PRG_DEBUG, __VA_ARGS__)
#define TRACE(...) LOG(PRG_TRACE, __VA_ARGS__)
