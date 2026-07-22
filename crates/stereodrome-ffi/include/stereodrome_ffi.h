#pragma once

#ifdef __cplusplus
extern "C" {
#endif

void stereodrome_core_free_string(char *value);
void stereodrome_core_set_log_callback(void (*callback)(const char *message));
void *stereodrome_core_new(const char *data_dir);
void stereodrome_core_destroy(void *core);
char *stereodrome_core_get_connection_status(void *core);
char *stereodrome_core_get_stream_uri(void *core, const char *song_id);
char *stereodrome_core_call(void *core, const char *method, const char *payload);
void *stereodrome_runtime_new(const char *data_dir);
void stereodrome_runtime_destroy(void *runtime);
char *stereodrome_runtime_dispatch(void *runtime, const char *command_json);
char *stereodrome_runtime_snapshot(void *runtime);
void stereodrome_runtime_set_event_callback(
    void *runtime,
    void (*callback)(const char *event, void *context),
    void *context);
void stereodrome_runtime_string_free(char *value);

#ifdef __cplusplus
}
#endif
