"""Keeps a live Snor session across separate tool calls.

`snor_ui_probe.py` drives the window but each invocation is its own process,
and launching Snor as a child of a short-lived shell gets it killed when the
shell exits. This script owns one long-lived process: it starts Snor, then
loops forever, so the app survives between probe invocations.

Usage:
    start     launch Snor detached from the terminal, then block forever
              (run this with run_in_background)
    stop      kill every Snor.exe

The child is created in its own process group (DETACHED_PROCESS) so that
killing this script does not take the app with it, and its stdout/stderr are
piped to a log file so a startup panic is readable instead of silent.
"""

import os
import subprocess
import sys
import time

ROOT = r"C:\My work folder\Snor"
EXE = os.path.join(ROOT, "target", "debug", "Snor.exe")
LOG = os.path.join(ROOT, ".workbuddy-ai", "verify", "snor.log")

DETACHED_PROCESS = 0x00000008
CREATE_NEW_PROCESS_GROUP = 0x00000200


def stop():
    subprocess.run(
        ["taskkill", "/IM", "Snor.exe", "/F"],
        capture_output=True,
        check=False,
    )


def start():
    os.makedirs(os.path.dirname(LOG), exist_ok=True)
    # A fresh log per launch, so a stale traceback cannot be mistaken for a
    # new one.
    with open(LOG, "w", encoding="utf-8") as fh:
        fh.write("")
    proc = subprocess.Popen(
        [EXE],
        cwd=ROOT,
        stdout=open(LOG, "a", encoding="utf-8"),
        stderr=subprocess.STDOUT,
        creationflags=DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP,
    )
    print(f"launched pid={proc.pid}", flush=True)
    # Block so this process stays alive; the child is detached and survives
    # its own parent only as long as nothing reaps the group, so keep the
    # script alive for the whole session and let the wait do that.
    try:
        while True:
            time.sleep(3600)
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "stop":
        stop()
        print("stopped")
    else:
        start()
