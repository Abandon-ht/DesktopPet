#!/usr/bin/env python3
"""Prepare clips under artifacts/voice/<locale>/<category>/<cue>.wav.

The imported recordings stay outside Git. Chinese source clips come from
artifacts/wav; other language matches are identified by the local ASR index.
Requires ffmpeg for converting the source Ogg/Vorbis files to PCM WAV.
"""
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]
CHINESE = ROOT / "artifacts/wav"
INDEX = Path.home() / "Downloads/wiki_audio/纳西妲_asr.txt"
SOURCES = INDEX.parent / "纳西妲"
TARGET = ROOT / "artifacts/voice"


def category(cue: str) -> str:
    if cue.startswith("intimacy_"):
        return "intimacy"
    if cue.startswith("feed_"):
        return "care"
    return "greetings"

ZH = {
    "first_meeting": "初次见面", "wake": "心事", "morning": "早上好",
    "noon": "午休时间到", "evening": "太阳落山", "night": "快去睡吧",
    "birthday": "生日", "feed_taste": "好味道", "feed_thought": "心意",
    "greeting": "去转转", "intimacy_smart": "变聪明啦",
    "intimacy_open": "思路变开阔了", "intimacy_feeling": "这种感觉",
    "intimacy_blessing": "赐福",
}

# Recording numbers are from the user's local ASR index. Omitted entries
# deliberately fall back to Chinese until the matching recording is reviewed.
OTHER = {
    "en-US": dict(first_meeting=98, wake=100, morning=222, noon=124,
                  evening=96, night=173, birthday=51, feed_taste=180,
                  feed_thought=159, greeting=80, intimacy_smart=59,
                  intimacy_open=201, intimacy_feeling=217, intimacy_blessing=256),
    "ja-JP": dict(first_meeting=89, noon=66, night=83, birthday=278,
                  feed_taste=164, feed_thought=45, intimacy_smart=168,
                  intimacy_open=61, intimacy_feeling=266, intimacy_blessing=120),
    "ko-KR": dict(first_meeting=280, wake=3, morning=275, noon=90,
                  night=53, birthday=71, feed_taste=178, feed_thought=12,
                  intimacy_smart=247, intimacy_open=265, intimacy_feeling=70,
                  intimacy_blessing=20),
}


def convert(source: Path, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.is_file() and destination.stat().st_mtime >= source.stat().st_mtime:
        return
    subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
                    "-i", str(source), "-ac", "1", "-ar", "24000",
                    "-c:a", "pcm_s16le", str(destination)], check=True)


def main() -> None:
    for cue, name in ZH.items():
        audio = CHINESE / f"{name}.mp3"
        transcript = CHINESE / f"{name}.txt"
        if not audio.is_file() or not transcript.is_file():
            raise SystemExit(f"missing Chinese source: {name}")
        destination = TARGET / "zh-CN" / category(cue) / cue
        convert(audio, destination.with_suffix(".wav"))
        shutil.copy2(transcript, destination.with_suffix(".txt"))

    rows = {}
    for line in INDEX.read_text(encoding="utf-8").splitlines():
        filename, transcript = line.split("\t", 1)
        rows[int(filename.split("_", 1)[0])] = (filename, transcript)
    for locale, cues in OTHER.items():
        for cue, number in cues.items():
            filename, transcript = rows[number]
            source = SOURCES / filename
            if not source.is_file():
                raise SystemExit(f"missing indexed recording: {source}")
            destination = TARGET / locale / category(cue) / cue
            convert(source, destination.with_suffix(".wav"))
            destination.with_suffix(".txt").write_text(transcript.strip() + "\n", encoding="utf-8")
    print(f"Prepared interaction clips in {TARGET}")


if __name__ == "__main__":
    main()
