cmake_minimum_required(VERSION 3.22.1)

project(stereodrome_core_jni)

add_library(stereodrome_ffi STATIC IMPORTED)
set_target_properties(
  stereodrome_ffi
  PROPERTIES
    IMPORTED_LOCATION
      ${CMAKE_CURRENT_LIST_DIR}/../../../rust-libs/${ANDROID_ABI}/libstereodrome_ffi.a
)

add_library(stereodrome_core_jni SHARED stereodrome_core_jni.cpp)

find_library(log-lib log)
find_library(android-lib android)
find_library(aaudio-lib aaudio)

target_link_libraries(
  stereodrome_core_jni
  stereodrome_ffi
  ${aaudio-lib}
  ${android-lib}
  ${log-lib}
)
