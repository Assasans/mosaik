#ifndef MOSAIK_DECODER_H
#define MOSAIK_DECODER_H

#include <unistd.h>
#include <memory>

extern "C" {
#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavfilter/buffersink.h>
#include <libavfilter/buffersrc.h>
#include <libswresample/swresample.h>
#include <libavutil/channel_layout.h>
#include <libavutil/opt.h>
}

#include "utils.h"

/// <div rustbindgen hide></div>
struct SwrContextDeleter {
  void operator()(SwrContext *context) const {
    if(context) swr_free(&context);
  }
};

/// <div rustbindgen hide></div>
struct AVPacketDeleter {
  void operator()(AVPacket *packet) const {
    if(packet) av_packet_free(&packet);
  }
};

/// <div rustbindgen hide></div>
struct AVFrameDeleter {
  void operator()(AVFrame *frame) const {
    if(frame) av_frame_free(&frame);
  }
};

/// <div rustbindgen hide></div>
struct AVFilterContextDeleter {
  void operator()(AVFilterContext *context) const {
    if(context) avfilter_free(context);
  }
};

/// <div rustbindgen hide></div>
struct AVFilterGraphDeleter {
  void operator()(AVFilterGraph *graph) const {
    if(graph) avfilter_graph_free(&graph);
  }
};

/// <div rustbindgen hide></div>
struct AVCodecContextDeleter {
  void operator()(AVCodecContext *context) const {
    if(context) avcodec_free_context(&context);
  }
};

/// <div rustbindgen hide></div>
struct AVFormatContextDeleter {
  void operator()(AVFormatContext *context) const {
    if(context) avformat_close_input(&context);
  }
};

/// <div rustbindgen hide></div>
struct AVFilterInOutDeleter {
  void operator()(AVFilterInOut *inout) const {
    if(inout) avfilter_inout_free(&inout);
  }
};

static void print_frame(const AVFrame *frame) {
  const int n = frame->nb_samples * frame->ch_layout.nb_channels;
  const uint32_t *p = (uint32_t *)frame->data[0];
  const uint32_t *p_end = p + n;

  while(p < p_end) {
    fputc((uint8_t)(*p & 0xff), stdout);
    fputc((uint8_t)(*p >> 8 & 0xff), stdout);
    fputc((uint8_t)(*p >> 16 & 0xff), stdout);
    fputc((uint8_t)(*p >> 24 & 0xff), stdout);
    p++;
  }
  fflush(stdout);
}

/// <div rustbindgen opaque></div>
class Decoder {
private:
  std::unique_ptr<AVFormatContext, AVFormatContextDeleter> fmt_ctx;
  std::unique_ptr<AVCodecContext, AVCodecContextDeleter> dec_ctx;
  AVFilterContext *buffersink_ctx;
  AVFilterContext *buffersrc_ctx;
  std::unique_ptr<AVFilterGraph, AVFilterGraphDeleter> filter_graph;
  bool enable_filter_graph = false;

  std::unique_ptr<AVPacket, AVPacketDeleter> packet;
  std::unique_ptr<AVFrame, AVFrameDeleter> frame;
  std::unique_ptr<AVFrame, AVFrameDeleter> out_frame;
  std::unique_ptr<AVFrame, AVFrameDeleter> filter_frame;
  std::unique_ptr<SwrContext, SwrContextDeleter> swr;

  int audio_stream_index = -1;

public:
  uint64_t pts = 0;
  uint64_t in_pts = 0;

  int init_filters(const char *filters_descr) {
    char args[512];
    int ret;
    const AVFilter *abuffersrc = avfilter_get_by_name("abuffer");
    const AVFilter *abuffersink = avfilter_get_by_name("abuffersink");
    AVFilterInOut *outputs = avfilter_inout_alloc();
    AVFilterInOut *inputs = avfilter_inout_alloc();
    static const enum AVSampleFormat out_sample_fmts[] = { AV_SAMPLE_FMT_FLT, AV_SAMPLE_FMT_NONE };
    static const int out_sample_rates[] = { 48000, -1 };
    const AVFilterLink *outlink;
    AVRational time_base = fmt_ctx->streams[audio_stream_index]->time_base;

    filter_graph = std::unique_ptr<AVFilterGraph, AVFilterGraphDeleter>(avfilter_graph_alloc());
    if(!outputs || !inputs || !filter_graph) {
      ret = AVERROR(ENOMEM);
      goto end;
    }

    /* buffer audio source: the decoded frames from the decoder will be inserted here. */
    if(dec_ctx->ch_layout.order == AV_CHANNEL_ORDER_UNSPEC)
      av_channel_layout_default(&dec_ctx->ch_layout, dec_ctx->ch_layout.nb_channels);
    ret = snprintf(args, sizeof(args), "time_base=%d/%d:sample_rate=%d:sample_fmt=%s:channel_layout=",
                   time_base.num, time_base.den, dec_ctx->sample_rate,
                   av_get_sample_fmt_name(dec_ctx->sample_fmt));
    av_channel_layout_describe(&dec_ctx->ch_layout, args + ret, sizeof(args) - ret);
    ret = avfilter_graph_create_filter(&buffersrc_ctx, abuffersrc, "in", args, nullptr, filter_graph.get());
    if(ret < 0) {
      av_log(nullptr, AV_LOG_ERROR, "Cannot create audio buffer source\n");
      goto end;
    }

    /* buffer audio sink: to terminate the filter chain. */
    ret = avfilter_graph_create_filter(&buffersink_ctx, abuffersink, "out", nullptr, nullptr, filter_graph.get());
    if(ret < 0) {
      av_log(nullptr, AV_LOG_ERROR, "Cannot create audio buffer sink\n");
      goto end;
    }

    ret = av_opt_set_int_list(buffersink_ctx, "sample_fmts", out_sample_fmts, AV_SAMPLE_FMT_NONE,
                              AV_OPT_SEARCH_CHILDREN);
    if(ret < 0) {
      av_log(nullptr, AV_LOG_ERROR, "Cannot set output sample format\n");
      goto end;
    }

    ret = av_opt_set(buffersink_ctx, "ch_layouts", "stereo", AV_OPT_SEARCH_CHILDREN);
    if(ret < 0) {
      av_log(nullptr, AV_LOG_ERROR, "Cannot set output channel layout\n");
      goto end;
    }

    ret = av_opt_set_int_list(buffersink_ctx, "sample_rates", out_sample_rates, -1, AV_OPT_SEARCH_CHILDREN);
    if(ret < 0) {
      av_log(nullptr, AV_LOG_ERROR, "Cannot set output sample rate\n");
      goto end;
    }

    /*
     * Set the endpoints for the filter graph. The filter_graph will
     * be linked to the graph described by filters_descr.
     */

    /*
     * The buffer source output must be connected to the input pad of
     * the first filter described by filters_descr; since the first
     * filter input label is not specified, it is set to "in" by
     * default.
     */
    outputs->name = av_strdup("in");
    outputs->filter_ctx = buffersrc_ctx;
    outputs->pad_idx = 0;
    outputs->next = nullptr;

    /*
     * The buffer sink input must be connected to the output pad of
     * the last filter described by filters_descr; since the last
     * filter output label is not specified, it is set to "out" by
     * default.
     */
    inputs->name = av_strdup("out");
    inputs->filter_ctx = buffersink_ctx;
    inputs->pad_idx = 0;
    inputs->next = nullptr;

    if((ret = avfilter_graph_parse_ptr(filter_graph.get(), filters_descr, &inputs, &outputs, nullptr)) < 0)
      goto end;

    if((ret = avfilter_graph_config(filter_graph.get(), nullptr)) < 0)
      goto end;

    /* Print summary of the sink buffer
     * Note: args buffer is reused to store channel layout string */
    outlink = buffersink_ctx->inputs[0];
    av_channel_layout_describe(&outlink->ch_layout, args, sizeof(args));
    av_log(nullptr, AV_LOG_INFO, "Output: srate:%dHz fmt:%s chlayout:%s\n",
           (int)outlink->sample_rate,
           (char *)av_x_if_null(av_get_sample_fmt_name(static_cast<AVSampleFormat>(outlink->format)), "?"),
           args);

    end:
    avfilter_inout_free(&inputs);
    avfilter_inout_free(&outputs);

    return ret;
  }

  int open_input(const char *path) {
    const AVCodec *dec;
    int ret;

    AVFormatContext *fmt_ctx_raw = nullptr;
    if((ret = avformat_open_input(&fmt_ctx_raw, path, nullptr, nullptr)) < 0) {
      av_log(nullptr, AV_LOG_ERROR, "Cannot open input file\n");
      return ret;
    }
    fmt_ctx = std::unique_ptr<AVFormatContext, AVFormatContextDeleter>(fmt_ctx_raw);

    fmt_ctx->flags |= AVIO_FLAG_NONBLOCK;

    if((ret = avformat_find_stream_info(fmt_ctx.get(), nullptr)) < 0) {
      av_log(nullptr, AV_LOG_ERROR, "Cannot find stream information\n");
      return ret;
    }

    /* select the audio stream */
    ret = av_find_best_stream(fmt_ctx.get(), AVMEDIA_TYPE_AUDIO, -1, -1, &dec, 0);
    if(ret < 0) {
      av_log(nullptr, AV_LOG_ERROR, "Cannot find an audio stream in the input file\n");
      return ret;
    }
    audio_stream_index = ret;

    ret = av_opt_set_int(fmt_ctx.get(), "reconnect", 1, AV_OPT_SEARCH_CHILDREN);
    if(ret < 0) {
      av_log(nullptr, AV_LOG_ERROR, "Cannot set reconnect\n");
      // return ret;
    }

    /* create decoding context */
    dec_ctx = std::unique_ptr<AVCodecContext, AVCodecContextDeleter>(avcodec_alloc_context3(dec));
    if(!dec_ctx)
      return AVERROR(ENOMEM);
    avcodec_parameters_to_context(dec_ctx.get(), fmt_ctx->streams[audio_stream_index]->codecpar);

    /* init the audio decoder */
    if((ret = avcodec_open2(dec_ctx.get(), dec, nullptr)) < 0) {
      av_log(nullptr, AV_LOG_ERROR, "Cannot open audio decoder\n");
      return ret;
    }

    return ret;
  }

  Decoder() {
    int ret;
    packet = std::unique_ptr<AVPacket, AVPacketDeleter>(av_packet_alloc());
    frame = std::unique_ptr<AVFrame, AVFrameDeleter>(av_frame_alloc());
    out_frame = std::unique_ptr<AVFrame, AVFrameDeleter>(av_frame_alloc());
    filter_frame = std::unique_ptr<AVFrame, AVFrameDeleter>(av_frame_alloc());
    swr = std::unique_ptr<SwrContext, SwrContextDeleter>(swr_alloc());

    if(!packet || !frame || !filter_frame) {
      fprintf(stderr, "Could not allocate frame or packet\n");
      exit(1);
    }

    if(isatty(STDOUT_FILENO)) {
      fprintf(stderr, "stdout is connected to tty\n");
      // exit(1);
    }
  }

  Decoder(const Decoder &other) = delete;

  int read_frame(void (*frame_callback)(float *data, int data_length, void* user), void* user) {
    int ret;
    if((ret = av_read_frame(fmt_ctx.get(), packet.get())) < 0) {
      av_log(nullptr, AV_LOG_ERROR, "Error while av_read_frame\n");
      goto end;
    }

    if(packet->stream_index == audio_stream_index) {
      ret = avcodec_send_packet(dec_ctx.get(), packet.get());
      if(ret < 0) {
        av_log(nullptr, AV_LOG_ERROR, "Error while sending a packet to the decoder\n");
        goto end;
      }

      while(ret >= 0) {
        ret = avcodec_receive_frame(dec_ctx.get(), frame.get());
        in_pts += frame->nb_samples;
        if(ret == AVERROR(EAGAIN) || ret == AVERROR_EOF) {
          // av_log(nullptr, AV_LOG_ERROR, "AGAIN or EOF while avcodec_receive_frame\n");
          break;
        } else if(ret < 0) {
          av_log(nullptr, AV_LOG_ERROR, "Error while receiving a frame from the decoder\n");
          goto end;
        }

        AVFrame *process_frame;
        if(enable_filter_graph) {
          /* push the audio data from decoded frame into the filtergraph */
          if(av_buffersrc_add_frame_flags(buffersrc_ctx, frame.get(), AV_BUFFERSRC_FLAG_KEEP_REF) < 0) {
            av_log(nullptr, AV_LOG_ERROR, "Error while feeding the audio filtergraph\n");
            break;
          }

          process_frame = filter_frame.get();
        } else {
          process_frame = frame.get();
        }

        /* pull filtered audio from the filtergraph */
        while(true) {
          if(enable_filter_graph) {
            ret = av_buffersink_get_frame(buffersink_ctx, filter_frame.get());
            if(ret == AVERROR(EAGAIN) || ret == AVERROR_EOF) break;
            if(ret < 0) {
              av_log(nullptr, AV_LOG_ERROR, "Error while av_buffersink_get_frame\n");
              goto end;
            }
          }

          out_frame->format = AV_SAMPLE_FMT_FLT;
          out_frame->ch_layout = AV_CHANNEL_LAYOUT_STEREO;
          out_frame->sample_rate = 48000;
          if(!swr_is_initialized(swr.get())) {
            fprintf(
              stderr,
              "Initializing libswresample: rate=%d, sample_fmt=%s\n",
              process_frame->sample_rate,
              av_get_sample_fmt_name((AVSampleFormat)process_frame->format)
            );

            SwrContext *swr_raw = swr.get();
            if((ret = swr_alloc_set_opts2(
              &swr_raw,
              &out_frame->ch_layout,
              (AVSampleFormat)out_frame->format,
              out_frame->sample_rate,
              &process_frame->ch_layout,
              (AVSampleFormat)process_frame->format,
              process_frame->sample_rate,
              0,
              nullptr
            )) < 0) {
              av_log(nullptr, AV_LOG_ERROR, "Error while swr_alloc_set_opts2\n");
              goto end;
            }

            if((ret = swr_init(swr.get())) < 0) {
              av_log(nullptr, AV_LOG_ERROR, "Error while swr_init\n");
              goto end;
            }
          }

          // This does the same thing as swr_convert_frame, but without the stupid config_changed call,
          // which returns an AVERROR_INPUT_CHANGED even if it is not a case.
          int out_num_samples = 48000;
          // av_rescale_rnd(swr_get_delay(swr.get(), process_frame->sample_rate) + process_frame->nb_samples, out_frame->sample_rate, process_frame->sample_rate, AV_ROUND_UP);
          // av_log(nullptr, AV_LOG_ERROR, "out=%d in=%d = %d\n", out_frame->sample_rate, process_frame->sample_rate, out_num_samples);
          out_frame->nb_samples = out_num_samples;
          // out_frame->nb_samples = swr_get_delay(swr.get(), out_frame->sample_rate)
          //                         + process_frame->nb_samples * (int64_t)out_frame->sample_rate / process_frame->sample_rate
          //                         + 3;
          if((ret = av_frame_get_buffer(out_frame.get(), 0)) < 0) {
            av_log(nullptr, AV_LOG_ERROR, "Error while av_frame_get_buffer\n");
            goto end;
          }
          if((ret = swr_convert(swr.get(), out_frame->extended_data, out_frame->nb_samples,
                                (const uint8_t **)process_frame->extended_data, process_frame->nb_samples)) < 0) {
            av_log(nullptr, AV_LOG_ERROR, "Error while swr_convert\n");
            goto end;
          }
          out_frame->nb_samples = ret;

          pts += out_frame->nb_samples;
          // printf("increment pts by %d -> %ld\n", out_frame->nb_samples, pts);

          const int n = out_frame->nb_samples * out_frame->ch_layout.nb_channels;
          auto data = reinterpret_cast<float *>(out_frame->extended_data[0]);
          frame_callback(data, n, user);

          // print_frame(out_frame.get());
          av_frame_unref(out_frame.get());
          av_frame_unref(process_frame);

          if(!enable_filter_graph) {
            break;
          }
        }
        av_frame_unref(frame.get());
      }
    }
    av_packet_unref(packet.get());

    end:

    if(ret < 0 && ret != AVERROR_EOF && ret != AVERROR(EAGAIN)) {
      fprintf(stderr, "Error occurred: %s\n", av_err2string(ret).c_str());
      exit(1);
    }

    return ret;
  }

  // TODO(Assasans): Some filters like "dynaudnorm" buffer data in filtergraph, we need to implement flushing from it.
  int flush_frame(void (*frame_callback)(float *data, int data_length, void* user), void* user) {
    int ret;

    out_frame->format = AV_SAMPLE_FMT_FLT;
    out_frame->ch_layout = AV_CHANNEL_LAYOUT_STEREO;
    out_frame->sample_rate = 48000;

    // This does the same thing as swr_convert_frame, but without the stupid config_changed call,
    // which returns an AVERROR_INPUT_CHANGED even if it is not a case.
    out_frame->nb_samples = 48000;
    if((ret = av_frame_get_buffer(out_frame.get(), 0)) < 0) {
      av_log(nullptr, AV_LOG_ERROR, "Error while av_frame_get_buffer\n");
      goto end;
    }

    if((ret = swr_convert(swr.get(), out_frame->extended_data, out_frame->nb_samples, nullptr, 0)) < 0) {
      av_log(nullptr, AV_LOG_ERROR, "Error while swr_convert (flush)\n");
      goto end;
    }
    out_frame->nb_samples = ret;

    pts += out_frame->nb_samples;

    {
      const int n = out_frame->nb_samples * out_frame->ch_layout.nb_channels;
      if(n < 1) {
        return AVERROR_EOF;
      }

      auto data = reinterpret_cast<float *>(out_frame->extended_data[0]);
      frame_callback(data, n, user);

      // print_frame(out_frame.get());
      av_frame_unref(out_frame.get());
    }

    end:

    if(ret < 0 && ret != AVERROR_EOF && ret != AVERROR(EAGAIN)) {
      fprintf(stderr, "Error occurred: %s\n", av_err2string(ret).c_str());
      exit(1);
    }

    return ret;
  }

  int unref_frame() {
    av_frame_unref(out_frame.get());
    return 0;
  }

  uint64_t get_frame_pts() {
    if(out_frame->pts != AV_NOPTS_VALUE) {
      // TODO(Assasans): Not tested
      av_log(nullptr, AV_LOG_ERROR, "get_frame_pts fast path");
      AVRational time_base = out_frame->time_base;
      return out_frame->pts * 1000 * time_base.num / time_base.den;
    }

    const int sample_rate = dec_ctx->sample_rate;
    // printf("get pts %ld, sample rate %d\n", pts, sample_rate);
    // printf("%lu\n", pts * 1000 / sample_rate);

    return in_pts * 1000 / sample_rate;
  }

  int get_decoder_time_base() {
    return dec_ctx->time_base.den;
  }

  int seek(int64_t pts) {
    AVRational decoder_time_base = dec_ctx->time_base;
    AVRational stream_time_base = fmt_ctx->streams[audio_stream_index]->time_base;

    // Original   : X * (n1 / d1) / (n2 / d2)
    // Without FP : X * n1 * d2 / d1 / n2
    int64_t timestamp =
      pts * decoder_time_base.num * stream_time_base.den / decoder_time_base.den / stream_time_base.num;

    int ret = av_seek_frame(fmt_ctx.get(), audio_stream_index, timestamp, AVSEEK_FLAG_ANY);
    avcodec_flush_buffers(dec_ctx.get());
    this->in_pts = pts;
    this->pts = 0; // TODO(Assasans): How to calculate it?

    // while(true) {
    //   if((ret = swr_convert_frame(swr.get(), nullptr, nullptr)) < 0) {
    //     av_log(nullptr, AV_LOG_ERROR, "Error while swr_convert_frame (flush)\n");
    //     fprintf(stderr, "Error occurred: %s\n", av_err2string(ret).c_str());
    //     break;
    //   }
    // }

    av_log(nullptr, AV_LOG_ERROR, "Seek to %ld -> %ld, stream_time_base: %d/%d\n", pts, timestamp, stream_time_base.num,
           stream_time_base.den);

    return ret;
  }

  int set_enable_filter_graph(bool enable) {
    bool changed = enable_filter_graph != enable;
    enable_filter_graph = enable;

    int ret = 0;
    if(changed && swr_is_initialized(swr.get())) {
      swr_close(swr.get());
    }

    return ret;
  }
};

DLL_EXPORT Decoder *decoder_alloc() {
  return new Decoder();
}

DLL_EXPORT void decoder_free(Decoder *decoder) {
  delete decoder;
}

DLL_EXPORT int decoder_open_input(Decoder *decoder, const char *path) {
  return decoder->open_input(path);
}

DLL_EXPORT int decoder_init_filters(Decoder *decoder, const char *filters_descr) {
  return decoder->init_filters(filters_descr);
}

DLL_EXPORT int decoder_read_frame(Decoder *decoder, void (*frame_callback)(float *data, int data_length, void* user), void* user) {
  return decoder->read_frame(frame_callback, user);
}

DLL_EXPORT int decoder_flush_frame(Decoder *decoder, void (*frame_callback)(float *data, int data_length, void* user), void* user) {
  return decoder->flush_frame(frame_callback, user);
}

DLL_EXPORT int decoder_unref_frame(Decoder *decoder) {
  return decoder->unref_frame();
}

DLL_EXPORT uint64_t decoder_get_frame_pts(Decoder *decoder) {
  return decoder->get_frame_pts();
}

DLL_EXPORT int decoder_get_decoder_time_base(Decoder *decoder) {
  return decoder->get_decoder_time_base();
}

DLL_EXPORT int decoder_seek(Decoder *decoder, uint64_t pts) {
  return decoder->seek(pts);
}

DLL_EXPORT int decoder_set_enable_filter_graph(Decoder *decoder, bool enable) {
  return decoder->set_enable_filter_graph(enable);
}

DLL_EXPORT int decoder_util_error_to_string(int error_code, char* buffer, int buffer_length) {
  return av_strerror(error_code, buffer, buffer_length);
}

DLL_EXPORT const int ERROR_MAX_STRING_SIZE = AV_ERROR_MAX_STRING_SIZE;

#endif
