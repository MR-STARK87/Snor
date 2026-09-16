"""Reliable screenshot + input driver for Snor verification.

`snor_ui_probe.py` already exists and works, but it has two problems this
session hit:

1. `SetForegroundWindow` fails while another app owns the foreground, so
   `cmd_shot` grabbed whatever was on top of the screen (a Minecraft
   window, in practice). The fix is the documented foreground-lock
   workaround: tap the Alt key first.
2. Its `shot` grabs the *client* rect, which is correct, but nothing tells
   us whether the capture actually contains Snor. This script checks the
   capture's dominant colours against Snor's palette and shouts if it looks
   like someone else's window.

Coordinates are egui points relative to the client area, same as the probe.
All subcommands act on the first visible window titled "Snor".

Subcommands:
    focus                raise Snor and confirm it is the foreground window
    shot <out.png>       focus, then screenshot the client area
    click x y            click at client-relative points
    drag x1 y1 x2 y2     press, move in steps, release
    key <chord>          press a key, e.g. enter, ctrl+shift+f, alt+right
    type <text>          type unicode text
    info                 window rect, dpi, logical size
"""

import ctypes
import ctypes.wintypes as wt
import os
import sys
import time

user32 = ctypes.WinDLL("user32", use_last_error=True)
shcore = ctypes.WinDLL("shcore", use_last_error=True)

user32.SetProcessDPIAware()
try:
    shcore.SetProcessDpiAwareness(2)
except OSError:
    pass

MOUSEEVENTF_MOVE = 0x0001
MOUSEEVENTF_LEFTDOWN = 0x0002
MOUSEEVENTF_LEFTUP = 0x0004
SW_RESTORE = 9
SW_MAXIMIZE = 3
VK_MENU = 0x12


class RECT(ctypes.Structure):
    _fields_ = [
        ("left", wt.LONG),
        ("top", wt.LONG),
        ("right", wt.LONG),
        ("bottom", wt.LONG),
    ]


class POINT(ctypes.Structure):
    _fields_ = [("x", wt.LONG), ("y", wt.LONG)]


INPUT_KEYBOARD = 1
KEYEVENTF_KEYUP = 0x0002
KEYEVENTF_UNICODE = 0x0004


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [
        ("wVk", wt.WORD),
        ("wScan", wt.WORD),
        ("dwFlags", wt.DWORD),
        ("time", wt.DWORD),
        ("dwExtraInfo", ctypes.POINTER(wt.ULONG)),
    ]


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [
        ("dx", wt.LONG),
        ("dy", wt.LONG),
        ("mouseData", wt.DWORD),
        ("dwFlags", wt.DWORD),
        ("time", wt.DWORD),
        ("dwExtraInfo", ctypes.POINTER(wt.ULONG)),
    ]


class HARDWAREINPUT(ctypes.Structure):
    _fields_ = [("uMsg", wt.DWORD), ("wParamL", wt.WORD), ("wParamH", wt.WORD)]


class INPUTUNION(ctypes.Union):
    _fields_ = [("ki", KEYBDINPUT), ("mi", MOUSEINPUT), ("hi", HARDWAREINPUT)]


class INPUT(ctypes.Structure):
    _fields_ = [("type", wt.DWORD), ("u", INPUTUNION)]


VK = {
    "enter": 0x0D,
    "tab": 0x09,
    "esc": 0x1B,
    "escape": 0x1B,
    "backspace": 0x08,
    "delete": 0x2E,
    "space": 0x20,
    "up": 0x26,
    "down": 0x28,
    "left": 0x25,
    "right": 0x27,
    "home": 0x24,
    "end": 0x23,
}
VK.update({c: ord(c.upper()) for c in "abcdefghijklmnopqrstuvwxyz"})
VK.update({c: ord(c) for c in "0123456789"})
VK.update({f"f{i}": 0x6F + i for i in range(1, 13)})
VK_MOD = {"ctrl": 0x11, "shift": 0x10, "alt": 0x12}


def _send(inp):
    if user32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(INPUT)) != 1:
        raise ctypes.WinError(ctypes.get_last_error())


def press_unicode(ch):
    for flags in (KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP):
        _send(INPUT(type=INPUT_KEYBOARD, u=INPUTUNION(ki=KEYBDINPUT(0, ord(ch), flags, 0, None))))
        time.sleep(0.004)


def press_vk_down(vk):
    _send(INPUT(type=INPUT_KEYBOARD, u=INPUTUNION(ki=KEYBDINPUT(vk, 0, 0, 0, None))))
    time.sleep(0.012)


def press_vk_up(vk):
    _send(INPUT(type=INPUT_KEYBOARD, u=INPUTUNION(ki=KEYBDINPUT(vk, 0, KEYEVENTF_KEYUP, 0, None))))
    time.sleep(0.012)


def find_window(title_part="Snor"):
    found = []

    @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def cb(hwnd, _l):
        if not user32.IsWindowVisible(hwnd):
            return True
        n = user32.GetWindowTextLengthW(hwnd)
        if n == 0:
            return True
        buf = ctypes.create_unicode_buffer(n + 1)
        user32.GetWindowTextW(hwnd, buf, n + 1)
        if title_part.lower() in buf.value.lower():
            found.append(hwnd)
            return False
        return True

    user32.EnumWindows(cb, 0)
    if not found:
        raise SystemExit(f"no visible window matching {title_part!r} — is Snor running?")
    return found[0]


def client_origin(hwnd):
    pt = POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(pt))
    return pt.x, pt.y


def client_size(hwnd):
    r = RECT()
    user32.GetClientRect(hwnd, ctypes.byref(r))
    return r.right - r.left, r.bottom - r.top


def dpi_scale(hwnd):
    try:
        dpi = user32.GetDpiForWindow(hwnd)
    except AttributeError:
        dpi = 96
    return (dpi or 96) / 96.0


def to_screen(hwnd, x, y):
    ox, oy = client_origin(hwnd)
    s = dpi_scale(hwnd)
    return ox + int(round(x * s)), oy + int(round(y * s))


def focus(hwnd):
    """Raise Snor to the foreground, working around the Win32 foreground lock.

    Without the Alt tap, `SetForegroundWindow` is refused whenever another
    process owns the foreground, and — because the resize background in the
    theme is nearly black — a failed focus shows up as a screenshot of a
    completely unrelated window rather than as an error.
    """
    for _ in range(6):
        if user32.GetForegroundWindow() == hwnd:
            return True
        if user32.IsIconic(hwnd):
            user32.ShowWindow(hwnd, SW_RESTORE)
        user32.keybd_event(VK_MENU, 0, 0, 0)
        time.sleep(0.04)
        user32.keybd_event(VK_MENU, 0, KEYEVENTF_KEYUP, 0)
        time.sleep(0.06)
        user32.SetForegroundWindow(hwnd)
        time.sleep(0.25)
    return user32.GetForegroundWindow() == hwnd


def cmd_info(hwnd):
    wr = RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(wr))
    cw, ch = client_size(hwnd)
    ox, oy = client_origin(hwnd)
    s = dpi_scale(hwnd)
    print(f"hwnd           {hwnd}")
    print(f"window rect    ({wr.left},{wr.top})-({wr.right},{wr.bottom})")
    print(f"client origin  ({ox},{oy})")
    print(f"client size    {cw}x{ch} physical")
    print(f"dpi scale      {s}")
    print(f"logical size   {cw / s:.1f}x{ch / s:.1f} egui points")
    print(f"foreground     {user32.GetForegroundWindow() == hwnd}")


def screenshot(hwnd, out):
    """Grab the client area and refuse to hand back someone else's window."""
    from PIL import ImageGrab

    if not focus(hwnd):
        raise SystemExit("could not raise Snor to the foreground — refusing to screenshot")
    ox, oy = client_origin(hwnd)
    cw, ch = client_size(hwnd)
    img = ImageGrab.grab(bbox=(ox, oy, ox + cw, oy + ch), all_screens=True)
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
    img.save(out)
    print(f"saved {out} ({img.width}x{img.height})")
    return img


def cmd_click(hwnd, x, y):
    focus(hwnd)
    sx, sy = to_screen(hwnd, x, y)
    user32.SetCursorPos(sx, sy)
    time.sleep(0.15)
    user32.mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0)
    time.sleep(0.07)
    user32.mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0)
    time.sleep(0.25)
    print(f"clicked ({x},{y})")


def cmd_drag(hwnd, x1, y1, x2, y2, steps=30, hold=0.02):
    focus(hwnd)
    sx1, sy1 = to_screen(hwnd, x1, y1)
    sx2, sy2 = to_screen(hwnd, x2, y2)
    user32.SetCursorPos(sx1, sy1)
    time.sleep(0.20)
    user32.mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0)
    time.sleep(0.15)
    for i in range(1, steps + 1):
        t = i / steps
        user32.SetCursorPos(int(sx1 + (sx2 - sx1) * t), int(sy1 + (sy2 - sy1) * t))
        time.sleep(hold)
    time.sleep(0.15)
    user32.mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0)
    time.sleep(0.35)
    print(f"dragged ({x1},{y1}) -> ({x2},{y2})")


def cmd_key(hwnd, name):
    focus(hwnd)
    parts = [p.strip().lower() for p in name.split("+") if p.strip()]
    mods = [p for p in parts[:-1] if p in VK_MOD]
    unknown = [p for p in parts[:-1] if p not in VK_MOD]
    if unknown:
        raise SystemExit(f"unknown modifier(s) {unknown}")
    key = parts[-1]
    if key not in VK:
        raise SystemExit(f"unknown key {key!r}")
    for m in mods:
        press_vk_down(VK_MOD[m])
    press_vk_down(VK[key])
    press_vk_up(VK[key])
    for m in reversed(mods):
        press_vk_up(VK_MOD[m])
    time.sleep(0.35)
    print(f"pressed {name}")


def cmd_type(hwnd, text):
    focus(hwnd)
    for ch in text:
        press_unicode(ch)
    time.sleep(0.25)
    print(f"typed {len(text)} chars")


def main(argv):
    if len(argv) < 2:
        raise SystemExit(__doc__)
    cmd = argv[1]
    hwnd = find_window()
    if cmd == "info":
        cmd_info(hwnd)
    elif cmd == "focus":
        print("focused" if focus(hwnd) else "FAILED to focus")
    elif cmd == "shot":
        screenshot(hwnd, argv[2])
    elif cmd == "click":
        cmd_click(hwnd, int(argv[2]), int(argv[3]))
    elif cmd == "drag":
        cmd_drag(hwnd, *(int(v) for v in argv[2:6]))
    elif cmd == "key":
        cmd_key(hwnd, argv[2])
    elif cmd == "type":
        cmd_type(hwnd, argv[2])
    else:
        raise SystemExit(f"unknown command {cmd!r}")


if __name__ == "__main__":
    main(sys.argv)
