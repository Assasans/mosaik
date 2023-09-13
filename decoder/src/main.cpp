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

int main(int argc, char **argv) {
  int ret;

  Decoder decoder { };
  if((ret = decoder.open_input("file:///run/media/assasans/D2C29497C2948201/Documents/REAPER Media/Катерина 2/3.mp3")) < 0) {
    av_log(nullptr, AV_LOG_ERROR, "Cannot open input\n");
    return ret;
  }

  if((ret = decoder.init_filters("aecho=0:1:1:0.5")) < 0) {
    av_log(nullptr, AV_LOG_ERROR, "Cannot initialize filter graph\n");
    return ret;
  }

  int x = 0;
  float* chunk = new float[4096];

  int length;
  while(true) {
    ret = decoder.read_frame(chunk, length);
    fprintf(stderr, "read %d\n", ret);
    if(ret < 0 && ret != AVERROR(EAGAIN)) break;

    x++;
    if(x == 100) {
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
    ret = decoder.flush_frame(chunk, length);
    fprintf(stderr, "flush %d\n", length);
    if(ret < 0 && ret != AVERROR(EAGAIN)) break;
  }

  return 0;
}
