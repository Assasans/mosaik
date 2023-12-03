cmake_minimum_required(VERSION 3.25)

project(mosaik-decoder)

set(CMAKE_MODULE_PATH ${CMAKE_CURRENT_SOURCE_DIR}/CMakeModules)
set(CMAKE_CXX_STANDARD 20)

add_library(${PROJECT_NAME} SHARED src/main.cpp src/api.cpp)
add_executable(${PROJECT_NAME}-test src/main.cpp)

set(FFMPEG_MINIMUM_VERSION 6.0)

find_package(PkgConfig REQUIRED)
pkg_check_modules(FFMPEG REQUIRED IMPORTED_TARGET libavformat libavcodec libavutil libavfilter libswresample)

if(PKG_CONFIG_FOUND)
  execute_process(
    COMMAND ${PKG_CONFIG_EXECUTABLE} --modversion libavformat
    OUTPUT_VARIABLE FFMPEG_VERSION
    OUTPUT_STRIP_TRAILING_WHITESPACE
  )
  message(STATUS "Found ffmpeg version: ${FFMPEG_VERSION}")
  if(FFMPEG_VERSION VERSION_LESS FFMPEG_MINIMUM_VERSION)
    message(FATAL_ERROR "ffmpeg version ${FFMPEG_VERSION} is below the minimum required version ${FFMPEG_MINIMUM_VERSION}")
  endif()
else()
  message(FATAL_ERROR "pkg-config is required to find ffmpeg")
endif()

target_link_libraries(${PROJECT_NAME} PUBLIC PkgConfig::FFMPEG)

target_compile_options(${PROJECT_NAME} PRIVATE
  $<$<CXX_COMPILER_ID:MSVC>:/W4 /WX>
  $<$<NOT:$<CXX_COMPILER_ID:MSVC>>:-Wall -Wextra -Wpedantic>
)

target_link_libraries(${PROJECT_NAME}-test PRIVATE ${PROJECT_NAME})

# target_compile_options(${PROJECT_NAME}-test PRIVATE -fsanitize=address)
# target_link_options(${PROJECT_NAME}-test PRIVATE -fsanitize=address)
