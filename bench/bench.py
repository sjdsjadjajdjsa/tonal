#!/usr/bin/env python3
"""Measures memory and CPU of desktop music players on Linux, the same way for each.

Each player starts fresh, sits idle, then plays music for a fixed time. Audio goes to a
silent PipeWire sink, so nothing is heard. Memory is PSS (proportional set size) summed
over every process the player starts, so shared libraries are counted fairly once.
CPU is the share of one core used by all those processes together.

Usage: bench/bench.py [player ...] [--runs N]   (results land in bench/results.json)
"""

import json
import os
import signal
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SONG = Path.home() / "Music/tonal-bench/song.mp3"
SINK = "bench_null"
SETTLE = 30
IDLE = 15
WARMUP = 15
PLAY = 30
TICK = os.sysconf("SC_CLK_TCK")


def tonal_env(settings=None):
    mpv = Path("/tmp/bench/mpv")
    mpv.mkdir(parents=True, exist_ok=True)
    (mpv / "mpv.conf").write_text(f"audio-device=pipewire/{SINK}\n")
    data = Path("/tmp/bench/tonal-data")
    subprocess.run(["rm", "-rf", str(data)])
    data.mkdir(parents=True)
    env = {"MPV_HOME": str(mpv), "XDG_DATA_HOME": str(data)}
    if settings is not None:
        # Let Tonal create its library, then write the settings under test.
        first = subprocess.Popen(PLAYERS["Tonal"]["cmd"], env=dict(os.environ, **env),
                                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(5)
        first.terminate()
        first.wait()
        db = data / "tonal/library.db"
        subprocess.run(["sqlite3", str(db), "DELETE FROM kv; INSERT INTO kv VALUES ('settings', '" + json.dumps(settings).replace("'", "''") + "')"])
    return env


PLAYERS = {
    "Tonal": {
        "cmd": [os.environ.get("TONAL_BIN", str(ROOT / "target/release/tonal"))],
        "env": tonal_env,
        "play_env": {"TONAL_DEBUG_SOURCE": "2", "TONAL_DEBUG_QUERY": "lofi chill"},
        "mpris": "tonal",
        "streams": True,
    },
    "Tonal (waves off)": {
        "cmd": [os.environ.get("TONAL_BIN", str(ROOT / "target/release/tonal"))],
        "env": lambda: tonal_env({"visualizer": False}),
        "play_env": {"TONAL_DEBUG_SOURCE": "2", "TONAL_DEBUG_QUERY": "lofi chill"},
        "mpris": "tonal",
        "streams": True,
    },
    "Tonal (live theme)": {
        "cmd": [os.environ.get("TONAL_BIN", str(ROOT / "target/release/tonal"))],
        "env": lambda: tonal_env({"theme": {"name": "Northern lights", "scene": "Aurora", "palette": "Boreal",
                                            "style": "Glass", "react": "Pulse", "seed": 11, "fps": 15}}),
        "play_env": {"TONAL_DEBUG_SOURCE": "2", "TONAL_DEBUG_QUERY": "lofi chill"},
        "mpris": "tonal",
        "streams": True,
    },
    "Spotify": {
        "cmd": ["spotify"],
        "mpris": "spotify",
        "uri": "spotify:track:0VjIjW4GlUZAMYd2vXMi3b",
        "streams": True,
    },
    # Electron apps run from their extracted AppImage (no FUSE 2 here), which is the same
    # binary the AppImage starts. Playback is started inside the page over DevTools.
    "YouTube Music (Pear)": {
        "cmd": ["/tmp/bench/squashfs-root/AppRun", "--no-sandbox", "--remote-debugging-port=9333"],
        "mpris": None,
        "devtools": (9333, [
            "location.href = 'https://music.youtube.com/watch?v=dQw4w9WgXcQ'; 1",
            "document.querySelector('video').play().then(() => 'ok')",
        ]),
        "streams": True,
    },
    "Riff": {
        "cmd": ["/tmp/bench/riffx/squashfs-root/AppRun", "--no-sandbox", "--remote-debugging-port=9334"],
        "mpris": None,
        "devtools": (9334, [
            "window.electronAPI.welcomeFinish ? (window.electronAPI.welcomeFinish({}), 'welcome') : 'main'",
            "document.querySelector('.my-wave-track-card').click(); 'ok'",
        ]),
        "streams": True,
    },
    "Strawberry": {
        "cmd": ["flatpak", "run", "org.strawberrymusicplayer.strawberry"],
        "mpris": "strawberry",
        "args_play": ["-a", str(SONG)],
    },
    "Amberol": {
        "cmd": ["flatpak", "run", "io.bassi.Amberol"],
        "mpris": "io.bassi.Amberol",
        "args_play": [str(SONG)],
    },
    "Tauon": {
        "cmd": ["flatpak", "run", "com.github.taiko2k.tauonmb"],
        "mpris": "tauon",
        "args_play": [str(SONG)],
    },
}


def children():
    tree = {}
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        try:
            stat = Path(f"/proc/{entry}/stat").read_text()
        except OSError:
            continue
        ppid = int(stat.rsplit(")", 1)[1].split()[1])
        tree.setdefault(ppid, []).append(int(entry))
    return tree


def descendants(root, extra_names=()):
    tree = children()
    found, stack = [], [root]
    while stack:
        pid = stack.pop()
        found.append(pid)
        stack.extend(tree.get(pid, []))
    # Flatpak and Electron can reparent helpers; also pick up processes by name.
    if extra_names:
        for entry in os.listdir("/proc"):
            if entry.isdigit() and int(entry) not in found:
                try:
                    cmd = Path(f"/proc/{entry}/cmdline").read_bytes().replace(b"\0", b" ").decode(errors="ignore")
                except OSError:
                    continue
                if any(n in cmd for n in extra_names):
                    found.append(int(entry))
    return found


def cpu_ticks(pids):
    total = {}
    for pid in pids:
        try:
            fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
            total[pid] = int(fields[11]) + int(fields[12])
        except (OSError, IndexError):
            pass
    return total


def pss_kb(pids):
    kb = 0
    for pid in pids:
        try:
            for line in Path(f"/proc/{pid}/smaps_rollup").read_text().splitlines():
                if line.startswith("Pss:"):
                    kb += int(line.split()[1])
        except OSError:
            pass
    return kb


def measure(root, names, seconds):
    pids = descendants(root, names)
    start, began = cpu_ticks(pids), time.monotonic()
    memory = []
    for _ in range(seconds):
        time.sleep(1)
        memory.append(pss_kb(descendants(root, names)))
    pids = descendants(root, names)
    end, elapsed = cpu_ticks(pids), time.monotonic() - began
    used = sum(end[p] - start.get(p, 0) for p in end)
    return {
        "cpu": round(100 * used / TICK / elapsed, 2),
        "mb": round(statistics.median(memory) / 1024, 1),
        "processes": len(pids),
    }


def mpris(name, method, *args):
    bus = f"org.mpris.MediaPlayer2.{name}"
    subprocess.run(
        ["dbus-send", "--session", "--type=method_call", f"--dest={bus}", "/org/mpris/MediaPlayer2",
         f"org.mpris.MediaPlayer2.Player.{method}", *args],
        capture_output=True,
    )


def status(name):
    names = subprocess.run(
        ["busctl", "--user", "list", "--no-legend"], capture_output=True, text=True
    ).stdout.split()
    bus = next((n for n in names if n.startswith(f"org.mpris.MediaPlayer2.{name}")), None)
    if not bus:
        return None
    out = subprocess.run(
        ["busctl", "--user", "get-property", bus, "/org/mpris/MediaPlayer2",
         "org.mpris.MediaPlayer2.Player", "PlaybackStatus"],
        capture_output=True, text=True,
    ).stdout
    return out.split('"')[1] if '"' in out else None


def devtools(port, steps):
    for step in steps:
        subprocess.run(["node", str(ROOT / "bench/cdp.mjs"), str(port), step], capture_output=True, timeout=30)
        time.sleep(10)


def sink_busy():
    out = subprocess.run(["pactl", "-f", "json", "list", "sink-inputs"], capture_output=True, text=True).stdout
    sinks = subprocess.run(["pactl", "-f", "json", "list", "sinks"], capture_output=True, text=True).stdout
    try:
        index = next(s["index"] for s in json.loads(sinks) if s["name"] == SINK)
        return any(i["sink"] == index and not i.get("corked") for i in json.loads(out))
    except (json.JSONDecodeError, StopIteration):
        return False


def playing(spec):
    if spec.get("mpris"):
        return status(spec["mpris"]) == "Playing"
    return sink_busy()


def silence(pids):
    """Moves every stream these processes play to the silent sink."""
    out = subprocess.run(["pactl", "-f", "json", "list", "sink-inputs"], capture_output=True, text=True).stdout
    try:
        inputs = json.loads(out)
    except json.JSONDecodeError:
        return
    for item in inputs:
        pid = item.get("properties", {}).get("application.process.id")
        if pid and int(pid) in pids:
            subprocess.run(["pactl", "move-sink-input", str(item["index"]), SINK], capture_output=True)


def run(name, spec):
    env = dict(os.environ, PULSE_SINK=SINK)
    if callable(spec.get("env")):
        env.update(spec["env"]())
    env.update(spec.get("play_env", {}))
    cmd = spec["cmd"] + spec.get("args_play", [])
    proc = subprocess.Popen(cmd, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    names = spec.get("names", ())
    deadline = time.monotonic() + SETTLE
    # Pause anything that starts by itself, so the idle phase really is idle.
    while time.monotonic() < deadline:
        silence(descendants(proc.pid, names))
        if spec.get("mpris") and status(spec["mpris"]) == "Playing":
            mpris(spec["mpris"], "Pause")
        time.sleep(0.3)
    idle = measure(proc.pid, names, IDLE)
    if "uri" in spec:
        mpris(spec["mpris"], "OpenUri", f"string:{spec['uri']}")
    elif "devtools" in spec:
        devtools(*spec["devtools"])
    else:
        mpris(spec["mpris"], "Play")
    deadline = time.monotonic() + WARMUP
    while time.monotonic() < deadline:
        silence(descendants(proc.pid, names))
        time.sleep(0.3)
    playing_ok = playing(spec)
    play = measure(proc.pid, names, PLAY)
    playing_ok = playing_ok and playing(spec)
    pids = descendants(proc.pid, names)
    os.killpg(proc.pid, signal.SIGTERM)
    for pid in pids:
        try:
            os.kill(pid, signal.SIGTERM)
        except OSError:
            pass
    time.sleep(3)
    for pid in descendants(proc.pid, names) + pids:
        try:
            os.kill(pid, signal.SIGKILL)
        except OSError:
            pass
    return {"idle": idle, "playing": play, "verified_playing": playing_ok}


def main():
    args = sys.argv[1:]
    runs = 2
    if "--runs" in args:
        i = args.index("--runs")
        runs = int(args[i + 1])
        del args[i : i + 2]
    chosen = args or list(PLAYERS)
    module = subprocess.run(
        ["pactl", "load-module", "module-null-sink", f"sink_name={SINK}"], capture_output=True, text=True
    ).stdout.strip()
    # Some players ignore PULSE_SINK and play to the default output, so make the
    # silent sink the default while measuring.
    # Remember the user's chosen output (not just the current one, which can be a
    # fallback while a monitor sleeps) and put it back afterwards.
    metadata = subprocess.run(["pw-metadata", "0", "default.configured.audio.sink"],
                              capture_output=True, text=True).stdout
    previous = metadata.split("value:'", 1)[1].split("'", 1)[0] if "value:'" in metadata else ""
    subprocess.run(["pactl", "set-default-sink", SINK])
    results_path = ROOT / "bench/results.json"
    results = json.loads(results_path.read_text()) if results_path.exists() else {}
    try:
        for name in chosen:
            samples = []
            for n in range(runs):
                sample = run(name, PLAYERS[name])
                print(name, n + 1, json.dumps(sample), flush=True)
                samples.append(sample)
                time.sleep(3)
            results[name] = {
                "streams": PLAYERS[name].get("streams", False),
                "runs": samples,
                **{
                    phase: {
                        key: statistics.median(s[phase][key] for s in samples)
                        for key in ("cpu", "mb", "processes")
                    }
                    for phase in ("idle", "playing")
                },
                "verified_playing": all(s["verified_playing"] for s in samples),
            }
            results_path.write_text(json.dumps(results, indent=2) + "\n")
    finally:
        if previous and SINK not in previous:
            subprocess.run(["pw-metadata", "0", "default.configured.audio.sink", previous], capture_output=True)
        if module:
            subprocess.run(["pactl", "unload-module", module], capture_output=True)


if __name__ == "__main__":
    main()
