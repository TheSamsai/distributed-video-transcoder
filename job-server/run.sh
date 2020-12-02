#!/usr/bin/env bash
set -euo pipefail

export FFMPEG_COMMAND="ffmpeg -i [input] -c:v libvpx-vp9 -b:v 2000k -crf 24 -speed 2 -c:a libopus -b:a 128k -f webm [output]"
export FILE_EXTENSION=".webm"
export COMPLETED_PATH="/home/sami/Ohjelmointi/Yliopisto/Distributed-Systems/distributed-video-transcoder/job-server/complete/"
export RSYNC_USER="sami"

cargo run
