#include <jni.h>
#include <stdint.h>

extern "C" {
void stereodrome_core_free_string(char *value);
void *stereodrome_core_new(const char *data_dir);
void stereodrome_core_destroy(void *core);
char *stereodrome_core_call(void *core, const char *method, const char *payload);
}

static jstring take_rust_string(JNIEnv *env, char *value) {
  if (value == nullptr) {
    return env->NewStringUTF("{\"ok\":false,\"error\":\"Rust returned null\"}");
  }

  jstring result = env->NewStringUTF(value);
  stereodrome_core_free_string(value);
  return result;
}

extern "C" JNIEXPORT jlong JNICALL
Java_expo_modules_stereodromecore_StereodromeCoreJni_nativeInitialize(
    JNIEnv *env, jobject, jstring data_dir) {
  const char *data_dir_chars = env->GetStringUTFChars(data_dir, nullptr);
  void *core = stereodrome_core_new(data_dir_chars);
  env->ReleaseStringUTFChars(data_dir, data_dir_chars);
  return reinterpret_cast<jlong>(core);
}

extern "C" JNIEXPORT void JNICALL
Java_expo_modules_stereodromecore_StereodromeCoreJni_nativeDestroy(
    JNIEnv *, jobject, jlong handle) {
  stereodrome_core_destroy(reinterpret_cast<void *>(handle));
}

extern "C" JNIEXPORT jstring JNICALL
Java_expo_modules_stereodromecore_StereodromeCoreJni_nativeCall(
    JNIEnv *env, jobject, jlong handle, jstring method, jstring payload) {
  const char *method_chars = env->GetStringUTFChars(method, nullptr);
  const char *payload_chars = env->GetStringUTFChars(payload, nullptr);
  char *result = stereodrome_core_call(
      reinterpret_cast<void *>(handle), method_chars, payload_chars);
  env->ReleaseStringUTFChars(method, method_chars);
  env->ReleaseStringUTFChars(payload, payload_chars);
  return take_rust_string(env, result);
}

