#include <jni.h>
#include <stdint.h>
#include <android/log.h>

extern "C" {
void stereodrome_core_free_string(char *value);
void *stereodrome_core_new(const char *data_dir);
void stereodrome_core_destroy(void *core);
char *stereodrome_core_call(void *core, const char *method, const char *payload);
char *stereodrome_runtime_dispatch(void *core, const char *command_json);
void stereodrome_core_set_log_callback(void (*callback)(const char *message));
void stereodrome_core_set_playback_callback(void (*callback)(const char *snapshot));
void stereodrome_core_set_event_callback(void (*callback)(const char *event));
}

static JavaVM *g_vm = nullptr;
static jclass g_bridge_class = nullptr;
static jmethodID g_playback_snapshot_method = nullptr;
static jmethodID g_core_event_method = nullptr;

static void rust_log_callback(const char *message) {
  if (message != nullptr) {
    __android_log_write(ANDROID_LOG_INFO, "StereodromeRust", message);
  }
}

static void rust_playback_callback(const char *snapshot) {
  if (snapshot == nullptr || g_vm == nullptr || g_bridge_class == nullptr ||
      g_playback_snapshot_method == nullptr) {
    return;
  }

  JNIEnv *env = nullptr;
  bool did_attach = false;
  jint env_result = g_vm->GetEnv(reinterpret_cast<void **>(&env), JNI_VERSION_1_6);
  if (env_result == JNI_EDETACHED) {
    if (g_vm->AttachCurrentThread(&env, nullptr) != JNI_OK) {
      return;
    }
    did_attach = true;
  } else if (env_result != JNI_OK || env == nullptr) {
    return;
  }

  jstring payload = env->NewStringUTF(snapshot);
  if (payload != nullptr) {
    env->CallStaticVoidMethod(g_bridge_class, g_playback_snapshot_method, payload);
    env->DeleteLocalRef(payload);
  }
  if (env->ExceptionCheck()) {
    env->ExceptionClear();
  }

  if (did_attach) {
    g_vm->DetachCurrentThread();
  }
}

static void rust_core_event_callback(const char *event) {
  if (event == nullptr || g_vm == nullptr || g_bridge_class == nullptr ||
      g_core_event_method == nullptr) {
    return;
  }

  JNIEnv *env = nullptr;
  bool did_attach = false;
  jint env_result = g_vm->GetEnv(reinterpret_cast<void **>(&env), JNI_VERSION_1_6);
  if (env_result == JNI_EDETACHED) {
    if (g_vm->AttachCurrentThread(&env, nullptr) != JNI_OK) {
      return;
    }
    did_attach = true;
  } else if (env_result != JNI_OK || env == nullptr) {
    return;
  }

  jstring payload = env->NewStringUTF(event);
  if (payload != nullptr) {
    env->CallStaticVoidMethod(g_bridge_class, g_core_event_method, payload);
    env->DeleteLocalRef(payload);
  }
  if (env->ExceptionCheck()) {
    env->ExceptionClear();
  }

  if (did_attach) {
    g_vm->DetachCurrentThread();
  }
}

static void cache_bridge_class(JNIEnv *env) {
  if (g_vm == nullptr) {
    env->GetJavaVM(&g_vm);
  }
  if (g_bridge_class != nullptr && g_playback_snapshot_method != nullptr &&
      g_core_event_method != nullptr) {
    return;
  }

  jclass local_bridge_class =
      env->FindClass("expo/modules/stereodromecore/StereodromeCoreBridge");
  if (local_bridge_class == nullptr) {
    env->ExceptionClear();
    return;
  }

  g_bridge_class = reinterpret_cast<jclass>(env->NewGlobalRef(local_bridge_class));
  env->DeleteLocalRef(local_bridge_class);
  if (g_bridge_class == nullptr) {
    return;
  }

  g_playback_snapshot_method =
      env->GetStaticMethodID(g_bridge_class, "onRustPlaybackSnapshot",
                             "(Ljava/lang/String;)V");
  if (g_playback_snapshot_method == nullptr) {
    env->ExceptionClear();
  }
  g_core_event_method =
      env->GetStaticMethodID(g_bridge_class, "onRustCoreEvent",
                             "(Ljava/lang/String;)V");
  if (g_core_event_method == nullptr) {
    env->ExceptionClear();
  }
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
  cache_bridge_class(env);
  stereodrome_core_set_log_callback(rust_log_callback);
  stereodrome_core_set_playback_callback(rust_playback_callback);
  stereodrome_core_set_event_callback(rust_core_event_callback);
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

extern "C" JNIEXPORT jstring JNICALL
Java_expo_modules_stereodromecore_StereodromeCoreJni_nativeDispatch(
    JNIEnv *env, jobject, jlong handle, jstring command_json) {
  const char *command_chars = env->GetStringUTFChars(command_json, nullptr);
  char *result = stereodrome_runtime_dispatch(
      reinterpret_cast<void *>(handle), command_chars);
  env->ReleaseStringUTFChars(command_json, command_chars);
  return take_rust_string(env, result);
}
