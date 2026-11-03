#!/bin/sh
# Builds the corpora tests play (needs ffmpeg):
# .nsh/tmp/zeo-player-test-music —— src/e2e.rs / tests/ui.rs 的曲库语料
#   （6 个音频 + lrc + png；文件数有断言，新格式样本别放这里）
# .nsh/tmp/zeo-player-test-formats —— 每种新格式一个 2 秒样本，
#   codec 测试用它验解码 / seek / 标签 / 波形
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
# 新格式样本：2 秒正弦波，标题写「格式 <主文件名>」。raw.aac 是 ADTS
# 裸流、容器没有标签位置，标题会回退成主文件名「raw」（测试按此断言）。
fmt_dir="$(dirname "$dir")/zeo-player-test-formats"
rm -rf "$fmt_dir"; mkdir -p "$fmt_dir"
ffmpeg -loglevel error -y -f lavfi -i "$tone" -t 2 -ac 2 -c:a aac -b:a 128k -metadata title="格式 aac" "$fmt_dir/aac.m4a"
ffmpeg -loglevel error -y -f lavfi -i "$tone" -t 2 -ac 2 -c:a alac -metadata title="格式 alac" "$fmt_dir/alac.m4a"
ffmpeg -loglevel error -y -f lavfi -i "$tone" -t 2 -ac 2 -c:a aac -b:a 128k -f adts "$fmt_dir/raw.aac"
ffmpeg -loglevel error -y -f lavfi -i "$tone" -t 2 -ac 2 -c:a libvorbis -q:a 4 -metadata title="格式 vorbis" "$fmt_dir/vorbis.ogg"
ffmpeg -loglevel error -y -f lavfi -i "$tone" -t 2 -ac 2 -c:a pcm_s16be -metadata title="格式 pcm" "$fmt_dir/pcm.aiff"
# 带 ReplayGain 标签的 FLAC：read_meta / read_replaygain / 增益测试用。
ffmpeg -loglevel error -y -f lavfi -i "$tone" -t 2 -ac 2 \
  -metadata title="格式 rg" \
  -metadata REPLAYGAIN_TRACK_GAIN="-6.00 dB" \
  -metadata REPLAYGAIN_TRACK_PEAK="0.5" \
  -metadata REPLAYGAIN_ALBUM_GAIN="-3.00 dB" "$fmt_dir/rg.flac"
echo "wrote $(find "$dir" -type f | wc -l | tr -d ' ') files to $dir"
echo "wrote $(find "$fmt_dir" -type f | wc -l | tr -d ' ') files to $fmt_dir"
