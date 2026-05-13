#pragma once

#ifdef __cplusplus
extern "C" {
#endif

void stereodrome_core_free_string(char *value);
void *stereodrome_core_new(const char *data_dir);
void stereodrome_core_destroy(void *core);
char *stereodrome_core_get_connection_status(void *core);
char *stereodrome_core_get_stream_uri(void *core, const char *song_id);
char *stereodrome_core_call(void *core, const char *method, const char *payload);

#ifdef __cplusplus
}
#endif

