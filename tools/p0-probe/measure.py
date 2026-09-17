#!/usr/bin/env python3
"""macOS process baseline; no third-party Python packages. This is not a Tauri supervisor."""
import argparse
import json
import os
from pathlib import Path
import statistics
import subprocess
import time


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("model", type=Path)
    parser.add_argument("--seconds", type=int, default=60)
    parser.add_argument("--output", type=Path, default=Path("artifacts/local/p0/measurement"))
    args = parser.parse_args()
    if not 1 <= args.seconds <= 3600:
        parser.error("seconds must be 1–3600")
    root = Path(__file__).resolve().parents[2]
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env.pop("P0_CAPTURE_DIR", None)
    # Whole-window passthrough keeps measurement from swallowing the user's clicks.
    env["P0_PASSTHROUGH"] = "1"
    command = [str(root / "target/release/p0-probe"), "window", str(args.model.resolve()), str(args.seconds)]
    samples = []
    started = time.monotonic()
    with (output / "host.stderr.log").open("w") as log:
        process = subprocess.Popen(command, env=env, stdout=subprocess.DEVNULL, stderr=log)
        try:
            while process.poll() is None:
                elapsed = time.monotonic() - started
                if elapsed > args.seconds + 30:
                    raise TimeoutError("host exceeded its auto-exit deadline")
                result = subprocess.run(["ps", "-p", str(process.pid), "-o", "%cpu=,rss="], capture_output=True, text=True, check=False)
                values = result.stdout.split()
                if len(values) == 2:
                    samples.append({"elapsed_s": elapsed, "cpu_percent": float(values[0]), "rss_kib": int(values[1])})
                time.sleep(2)
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
    steady = [s for s in samples if s["elapsed_s"] >= 5]
    events = []
    for line in (output / "host.stderr.log").read_text().splitlines():
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError:
            pass
    summaries = [e for e in events if e.get("event") == "summary"]
    report = {
        "command": command, "exit_code": process.returncode,
        "scope": "single native host, static pose between 3-second expression changes; capture disabled; excludes Tauri, WebView, physics, audio and LLM",
        "samples": samples, "events": events,
        "steady_after_s": 5,
        "cpu_percent_median": statistics.median(s["cpu_percent"] for s in steady) if steady else None,
        "rss_mib_median": statistics.median(s["rss_kib"] / 1024 for s in steady) if steady else None,
        "rss_mib_max": max((s["rss_kib"] / 1024 for s in samples), default=None),
        "stability_30_minutes_observed": bool(process.returncode == 0 and summaries and summaries[-1]["elapsed_s"] >= 1800),
    }
    (output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({k: v for k, v in report.items() if k not in ("samples", "events")}, ensure_ascii=False, indent=2))
    if process.returncode or not summaries:
        raise SystemExit("measurement failed; inspect host.stderr.log")


if __name__ == "__main__":
    main()
