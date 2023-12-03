#include <unistd.h>

extern "C" {
#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavfilter/buffersink.h>
#include <libavfilter/buffersrc.h>
#include <libswresample/swresample.h>
#include <libavutil/channel_layout.h>
#include <libavutil/opt.h>
}

#include "Decoder.h"

static void print_frame(const float *frame, const int n) {
  const uint32_t *p = (uint32_t *)frame;
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

int main(int argc, char **argv) {
  int ret;

  Decoder decoder { };
  if((ret = decoder.open_input("file:///home/assasans/Downloads/eimusics.comDamewaDameFLAC/01.Dame wa Dame.flac")) < 0) {
  // if((ret = decoder.open_input("file:///home/assasans/Videos/sumeru.ogg")) < 0) {
  // if((ret = decoder.open_input("sumeru1ch.wav")) < 0) {
    av_log(nullptr, AV_LOG_ERROR, "Cannot open input\n");
    return ret;
  }

  decoder.set_enable_filter_graph(true);
  if((ret = decoder.init_filters("lv2=p=http\\\\://drobilla.net/plugins/mda/Vocoder,lv2=p=http\\\\://calf.sourceforge.net/plugins/BassEnhancer")) < 0) {
    // if((ret = decoder.init_filters("aecho=0:1:1:0.5")) < 0) {
    av_log(nullptr, AV_LOG_ERROR, "Cannot initialize filter graph\n");
    decoder.set_enable_filter_graph(false);
    // return ret;
  }

  int x = 0;

  float* chunk;
  int length;
  while(true) {
    length = 0;
    ret = decoder.read_frame(chunk, length);
    if(ret >= 0) fprintf(stderr, "read %d, length %d\n", ret, length);
    if(ret < 0 && ret != AVERROR(EAGAIN)) break;

    print_frame(chunk, length);
    decoder.unref_frame();

    x++;
    if(x % 60 == 0) {
      decoder.set_enable_filter_graph(x % 120 == 0);
      fprintf(stderr, "pts %ld\n", decoder.get_frame_pts());
      // decoder.seek(decoder.in_pts + (44100 * 10));
      // if((ret = decoder.open_input("https://rr3---sn-qo5-2vgz.googlevideo.com/videoplayback?expire=1689979512&ei=GLa6ZKT4GdW7v_IPyZqvyAk&ip=176.93.44.73&id=o-ABg6wl_wgXrhSC15hcwFifZVQzEm52iWqCNCBByvzY6C&itag=251&source=youtube&requiressl=yes&mh=eG&mm=31%2C29&mn=sn-qo5-2vgz%2Csn-ixh7yn7d&ms=au%2Crdu&mv=m&mvi=3&pl=21&gcr=fi&initcwndbps=998750&spc=Ul2Sq5LorIJL7rLqEeVZtjzl7kKoz3c&vprv=1&svpuc=1&mime=audio%2Fwebm&gir=yes&clen=4601399&dur=264.281&lmt=1574053392219015&mt=1689957692&fvip=3&keepalive=yes&fexp=24007246%2C24362687&c=ANDROID&txp=2301222&sparams=expire%2Cei%2Cip%2Cid%2Citag%2Csource%2Crequiressl%2Cgcr%2Cspc%2Cvprv%2Csvpuc%2Cmime%2Cgir%2Cclen%2Cdur%2Clmt&sig=AOq0QJ8wRgIhAOhggNJ6vC2pYZZK4bTxJN7LhP8_cQiqVOcAGUjCHDNsAiEA370qHqq6DdxrsDs6Kq_WjyoEi3W5PrAkT-yhoF9AzC4%3D&lsparams=mh%2Cmm%2Cmn%2Cms%2Cmv%2Cmvi%2Cpl%2Cinitcwndbps&lsig=AG3C_xAwRQIgRZn_yPvtPD2Bg7doe_oIBQv7YHNecv2ivXZOE4V6H2oCIQC3PfVy5_2gf7T6aDT0CUUKTLreOwJ-IWtgylOP8AiBZQ%3D%3D")) < 0) {
      //   av_log(nullptr, AV_LOG_ERROR, "Cannot open input\n");
      //   return ret;
      // }
      //   if((ret = decoder.init_filters("lv2=p=http\\\\://calf.sourceforge.net/plugins/BassEnhancer:c=amount=20,alimiter=limit=0.891251,dynaudnorm")) < 0) {
      //     av_log(nullptr, AV_LOG_ERROR, "Cannot initialize filter graph\n");
      //     return ret;
      //   }
    }
  }

  while(true) {
    av_log(nullptr, AV_LOG_ERROR, "FLUSHING\n");

    ret = decoder.flush_frame(chunk, length);
    fprintf(stderr, "flush %d\n", length);
    if(ret < 0 && ret != AVERROR(EAGAIN)) break;

    decoder.unref_frame();
  }

  return 0;
}
