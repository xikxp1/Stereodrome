#pragma once

#ifdef __cplusplus
extern "C" {
#endif

void *stereodrome_runtime_new(const char *data_dir);
void stereodrome_runtime_destroy(void *runtime);
char *stereodrome_runtime_dispatch(void *runtime, const char *command_json);
char *stereodrome_runtime_snapshot(void *runtime);
void stereodrome_runtime_set_log_callback(void (*callback)(const char *message));
void stereodrome_runtime_set_event_callback(
    void *runtime,
    void (*callback)(const char *event, void *context),
    void *context);
void stereodrome_string_free(char *value);

#ifdef __cplusplus
}
#endif
