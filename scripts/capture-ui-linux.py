import json
import os
import sqlite3
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def create_fixture(db_path, theme):
    category = "7596a7aa-9ff3-49b9-988b-687797ee939e"
    periods = {}
    for index, start, end, note in [
        (1, "11:00", "11:25", "Короткая запись"),
        (2, "09:30", "10:15", "Подготовка материалов к обсуждению проекта, сводка и следующие шаги"),
    ]:
        periods[f"00000000-0000-0000-0000-{index:012}"] = {
            "start": f"2026-10-12T{start}:00Z",
            "end": f"2026-10-12T{end}:00Z",
            "note": note,
            "category": category,
            "kind": "work",
            "pauses": [],
        }
    state = {
        "dataset": {
            "categories": {category: {"name": "Продукт", "color": "#1f5c4b"}},
            "periods": periods,
            "zones": {},
            "settings": {"theme": theme},
        },
        "timer": None,
        "next_kind": "work",
        "cloud": {"setup": "Local"},
    }
    with sqlite3.connect(db_path) as db:
        db.execute(
            "CREATE TABLE local_state (id INTEGER PRIMARY KEY, generation INTEGER, body TEXT)"
        )
        db.execute("INSERT INTO local_state VALUES (1, 1, ?)", (json.dumps(state),))


def capture(binary, output, directory, width, height, scale, theme):
    db_path = directory / f"{width}-{theme}.sqlite3"
    create_fixture(db_path, theme)
    environment = dict(
        os.environ, TRACKING_DB=str(db_path), WINIT_X11_SCALE_FACTOR=str(scale), TZ="UTC"
    )
    environment.pop("WAYLAND_DISPLAY", None)
    environment.pop("WAYLAND_SOCKET", None)
    app = subprocess.Popen([binary], env=environment)
    try:
        window = subprocess.check_output(
            ["xdotool", "search", "--sync", "--onlyvisible", "--pid", str(app.pid)],
            text=True,
            timeout=20,
        ).splitlines()[0]
        pixels = (str(round(width * scale)), str(round(height * scale)))
        subprocess.run(
            ["xdotool", "windowsize", "--sync", window, *pixels], check=True, timeout=10
        )
        for screen in ["timer", "history"]:
            if screen == "history":
                subprocess.run(["xdotool", "windowfocus", "--sync", window], check=True, timeout=10)
                subprocess.run(["xdotool", "key", "Tab", "Tab", "Return"], check=True, timeout=10)
            time.sleep(0.6)
            path = output / f"desktop-{screen}-{width}-{theme}-{scale:g}x.png"
            subprocess.run(["scrot", "-o", "-w", window, path], check=True, timeout=10)
            print(path)
    finally:
        app.terminate()
        try:
            app.wait(timeout=5)
        except subprocess.TimeoutExpired:
            app.kill()
            app.wait()


def main():
    if not os.environ.get("DISPLAY"):
        raise SystemExit("Run this script inside xvfb-run.")
    repository = Path(__file__).resolve().parents[1]
    binary = repository / "target/debug/tracking-desktop"
    if not binary.is_file():
        raise SystemExit("Build the desktop binary with cargo build --locked -p app-desktop.")
    output = (
        Path(sys.argv[1]) if len(sys.argv) > 1 else repository / "target/ui-screenshots"
    ).resolve()
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as directory:
        for width, height, scale in [(1100, 820, 1.0), (360, 760, 1.25)]:
            for theme in ["light", "dark"]:
                capture(binary, output, Path(directory), width, height, scale, theme)


if __name__ == "__main__":
    main()
