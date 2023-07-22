#ifndef MOSAIK_DECODER_UTILS_H
#define MOSAIK_DECODER_UTILS_H

#ifdef av_err2str
#undef av_err2str

#include <string>

av_always_inline std::string av_err2string(int errnum) {
  char str[AV_ERROR_MAX_STRING_SIZE];
  return av_make_error_string(str, AV_ERROR_MAX_STRING_SIZE, errnum);
}

#define av_err2str(err) av_err2string(err).c_str()
#endif

#if defined(_MSC_VER)
#define DLL_EXPORT __declspec(dllexport) // Microsoft
#elif defined(__GNUC__)
#define DLL_EXPORT __attribute__((visibility("default"))) // GCC
#else
#define DLL_EXPORT // Most compilers export all the symbols by default. We hope for the best here.
#pragma warning Unknown dynamic link import/export semantics.
#endif

#endif
