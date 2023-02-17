typedef void (*on_connected_cb)(int32_t, void *);

typedef struct Options {
  const char *server;
  const char *cookie;
  const char *script;
  void *user_data;
} Options;

int start(const Options *options, on_connected_cb cb);
void stop();
