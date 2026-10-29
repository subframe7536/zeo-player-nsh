#!/bin/sh
# Builds the corpus src/e2e.rs plays: /tmp/zeo-test-music (needs ffmpeg).
# 6 files (mp3 / flac / wav, one in a subfolder), tagged, with an external .lrc.
set -eu
dir="${1:-/tmp/zeo-test-music}"
rm -rf "$dir"; mkdir -p "$dir/sub"
gen() { ffmpeg -loglevel error -y -f lavfi -i "$1" -t "$2" "$@"; }
tone="sine=frequency=440:sample_rate=44100"
quiet="sine=frequency=220:sample_rate=48000,volume=0.2"
ffmpeg -loglevel error -y -f lavfi -i "$tone" -t 2 -ac 2 \
  -metadata title="测试歌曲" -metadata artist="测试歌手 A; 测试歌手 B" -metadata album="测试专辑" \
  -metadata genre="Rock; Indie; Dream Pop" -metadata track=1 "$dir/sine.flac"
ffmpeg -loglevel error -y -f lavfi -i "$tone" -t 2 -ac 2 "$dir/sine.wav"
ffmpeg -loglevel error -y -f lavfi -i "$quiet" -t 3 -ac 2 -b:a 128k "$dir/quiet.mp3"
ffmpeg -loglevel error -y -f lavfi -i "$quiet" -t 3 -ac 2 "$dir/quiet.wav"
ffmpeg -loglevel error -y -f lavfi -i "$tone" -t 1 -ac 1 -b:a 96k "$dir/sub/mono.mp3"
ffmpeg -loglevel error -y -f lavfi -i "$quiet" -t 1 -ac 2 "$dir/sub/short.flac"
cat > "$dir/sine.lrc" <<'LRC'
[00:00.00]作词：甲
[00:00.00]作曲：乙
[00:00.00]编曲：丙
[00:01.00]第一句歌词
[00:01.00]First line
[00:05.00]
[00:12.00]间奏后的句子
[00:15.00]最后一句 <00:15.50>呀
LRC
echo "wrote $(find "$dir" -type f | wc -l | tr -d ' ') files to $dir"
