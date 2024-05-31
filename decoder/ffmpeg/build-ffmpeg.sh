#!/usr/bin/sh

set -e

REPO_URL="https://github.com/FFmpeg/FFmpeg"
LOCAL_DIR="src"
# Clone the repository if the directory does not exist
# If the directory exists, update the repository
git -C "$LOCAL_DIR" pull || git clone "$REPO_URL" "$LOCAL_DIR" && git -C "$LOCAL_DIR" fetch --all && git -C "$LOCAL_DIR" reset --hard origin/master

pushd src
# rm -rv ../bin
./configure \
  --disable-everything \
  --enable-decoder=flac \
  --enable-decoder=mp3 \
  --enable-decoder=opus \
  --enable-demuxer=flac \
  --enable-demuxer=mp3 \
  --enable-demuxer=matroska \
  --enable-protocol=file \
  --enable-protocol=http \
  --enable-protocol=https \
  --enable-openssl \
  --enable-version3 \
  --enable-shared \
  --enable-filters \
  --enable-lv2 \
  --enable-swresample \
  --enable-librubberband \
  --enable-libmysofa \
  --enable-libbs2b \
  --enable-gpl \
  --prefix="$(realpath ../bin)"
make -j$(nproc --ignore 8)
make install
popd
