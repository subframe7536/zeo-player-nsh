#!/bin/sh
# Builds the corpus src/e2e.rs and tests/ui.rs play:
# .nsh/tmp/zeo-player-test-music in the repo (needs ffmpeg).
# 6 files (mp3 / flac / wav, one in a subfolder), tagged, with an external .lrc.
set -eu
dir="${1:-$(dirname "$0")/../.nsh/tmp/zeo-player-test-music}"
rm -rf "$dir"; mkdir -p "$dir/sub"
gen() { ffmpeg -loglevel error -y -f lavfi -i "$1" -t "$2" "$@"; }
tone="sine=frequency=440:sample_rate=44100"
quiet="sine=frequency=220:sample_rate=48000,volume=0.2"
# Embedded red cover on sine.flac — the「跟随封面」accent tests need art_hue.
# 两遍：先写 2 秒音频，再把封面作为 attached_pic 复用流拷贝附进去
# （一遍过会被图片输入把音频流提前截断）。
ffmpeg -loglevel error -y -f lavfi -i "color=c=0xd0342c:size=64x64" -frames:v 1 "$dir/.cover.png"
# 10 秒：给「跟随封面」的播放测试留够窗口。
ffmpeg -loglevel error -y -f lavfi -i "$tone" -t 10 -ac 2 \
  -metadata title="测试歌曲" -metadata artist="测试歌手 A; 测试歌手 B" -metadata album="测试专辑" \
  -metadata genre="Rock; Indie; Dream Pop" -metadata track=1 "$dir/sine.flac"
ffmpeg -loglevel error -y -i "$dir/sine.flac" -i "$dir/.cover.png" \
  -map 0:a -map 1:v -c copy -disposition:v:0 attached_pic "$dir/.sine.flac"
mv "$dir/.sine.flac" "$dir/sine.flac"; rm "$dir/.cover.png"
# 同名本地封面 sine.png（蓝色，与内嵌红色区分）：「本地图片优先」时应取它。
ffmpeg -loglevel error -y -f lavfi -i "color=c=0x2c50d0:size=64x64" -frames:v 1 "$dir/sine.png"
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
