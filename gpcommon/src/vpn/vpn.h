#include <stdio.h>
#include <stdlib.h>
#include <stdarg.h>
#include <openconnect.h>

typedef struct vpn_options
{
    const char *server;
    const char *cookie;
    const char *script;
    void *user_data;
} vpn_options;

int vpn_connect(const vpn_options *options);
void vpn_disconnect();

extern void on_vpn_connected(int cmd_pipe_fd, void *user_data);
extern void vpn_log(int level, const char *msg);

static char *format_message(const char *format, va_list args)
{
    va_list args_copy;
    va_copy(args_copy, args);
    int len = vsnprintf(NULL, 0, format, args_copy);
    va_end(args_copy);

    char *buffer = malloc(len + 1);
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