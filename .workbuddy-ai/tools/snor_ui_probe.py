"""Windows UI probe for verifying Snor's GUI behavior.

Snor is an egui/eframe app, so its clicks, drags and pixels cannot be verified
headlessly. This script drives the real window through user32 so a fix can be
checked instead of assumed.

Subcommands:
    info                 print window/client rect, dpi scale, focus state
    shot <out.png>       focus the window and screenshot the client area
    drag x1 y1 x2 y2     press at (x1,y1), move in steps, release at (x2,y2)
    click x y            single left click
    move x y             move the cursor without pressing
    place cw ch sx sy    resize+move the window (physical px)
    maximize             fill the monitor work area
    type <text>          type text into the focused window (unicode)
    key <name>           press one key, optionally chorded: enter, ctrl+tab, ctrl+b

All coordinates are egui points relative to the client area's top-left corner.
The script converts them to physical screen pixels using the window's DPI.
"""

import ctypes
import ctypes.wintypes as wt
import sys
import time

user32 = ctypes.WinDLL("user32", use_last_error=True)
shcore = ctypes.WinDLL("shcore", use_last_error=True)

MOUSEEVENTF_MOVE = 0x0001
MOUSEEVENTF_LEFTDOWN = 0x0002
MOUSEEVENTF_LEFTUP = 0x0004
SW_RESTORE = 9
SW_MAXIMIZE = 3

user32.SetProcessDPIAware()
try:
    shcore.SetProcessDpiAwareness(2)  # PER_MONITOR_AWARE_V2
except OSError:
    pass


class RECT(ctypes.Structure):
    _fields_ = [
        ("left", wt.LONG),
        ("top", wt.LONG),
        ("right", wt.LONG),
        ("bottom", wt.LONG),
    ]


class POINT(ctypes.Structure):
    _fields_ = [("x", wt.LONG), ("y", wt.LONG)]


# --- SendInput plumbing -----------------------------------------------------
# Typing has to go through SendInput rather than keybd_event: keybd_event cannot
# emit characters that have no virtual-key code, and the terminal's input path
# reads egui Text events, which only appear for real translated keystrokes.

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
    _fields_ = [
        ("uMsg", wt.DWORD),
        ("wParamL", wt.WORD),
        ("wParamH", wt.WORD),
    ]


class INPUTUNION(ctypes.Union):
    _fields_ = [("ki", KEYBDINPUT), ("mi", MOUSEINPUT), ("hi", HARDWAREINPUT)]


class INPUT(ctypes.Structure):
    _fields_ = [("type", wt.DWORD), ("u", INPUTUNION)]


# Named keys, as virtual-key codes. Unicode typing covers everything else.
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


def _send(inp):
    if user32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(INPUT)) != 1:
        raise ctypes.WinError(ctypes.get_last_error())


def press_unicode(ch):
    """Type one character as a unicode keystroke (down then up)."""
    for flags in (KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP):
        _send(
            INPUT(
                type=INPUT_KEYBOARD,
                u=INPUTUNION(ki=KEYBDINPUT(0, ord(ch), flags, 0, None)),
            )
        )
        time.sleep(0.004)


def press_vk(vk):
    """Press and release a virtual key."""
    press_vk_down(vk)
    press_vk_up(vk)


def press_vk_down(vk):
    _send(
        INPUT(
            type=INPUT_KEYBOARD,
            u=INPUTUNION(ki=KEYBDINPUT(vk, 0, 0, 0, None)),
        )
    )
    time.sleep(0.012)


def press_vk_up(vk):
    _send(
        INPUT(
            type=INPUT_KEYBOARD,
            u=INPUTUNION(ki=KEYBDINPUT(vk, 0, KEYEVENTF_KEYUP, 0, None)),
        )
    )
    time.sleep(0.012)


def cmd_type(hwnd, text):
    focus(hwnd)
    for ch in text:
        press_unicode(ch)
    time.sleep(0.10)


VK_MOD = {"ctrl": 0x11, "shift": 0x10, "alt": 0x12}


def cmd_key(hwnd, name):
    """Press a key, optionally with modifiers: `enter`, `ctrl+tab`, `ctrl+b`.

    Modifiers are held down around the key so the app sees a real chord. Sending
    them as separate presses does not work: egui reads modifier state from the
    key event itself, so an unheld Ctrl arrives as a bare key.
    """
    focus(hwnd)
    parts = [p.strip().lower() for p in name.split("+") if p.strip()]
    if not parts:
        raise SystemExit("empty key")
    mods = [p for p in parts[:-1] if p in VK_MOD]
    unknown = [p for p in parts[:-1] if p not in VK_MOD]
    if unknown:
        raise SystemExit(f"unknown modifier(s) {unknown}; known: {', '.join(VK_MOD)}")
    key = parts[-1]
    if key not in VK:
        raise SystemExit(f"unknown key {key!r}; known: {', '.join(sorted(VK))}")
    for m in mods:
        press_vk_down(VK_MOD[m])
    press_vk(VK[key])
    for m in reversed(mods):
        press_vk_up(VK_MOD[m])
    time.sleep(0.12)


def find_window(title_part="Snor"):
    """Return the hwnd of the first visible top-level window matching the title."""
    found = []

    @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def cb(hwnd, _lparam):
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
    """Screen coords of the client area's top-left corner."""
    pt = POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(pt))
    return pt.x, pt.y


def client_size(hwnd):
    r = RECT()
    user32.GetClientRect(hwnd, ctypes.byref(r))
    return r.right - r.left, r.bottom - r.top


def dpi_scale(hwnd):
    """Physical pixels per egui point for this window."""
    try:
        dpi = user32.GetDpiForWindow(hwnd)
    except AttributeError:
        dpi = 96
    return (dpi or 96) / 96.0


def to_screen(hwnd, x, y):
    """egui points (client-relative) -> physical screen pixels."""
    ox, oy = client_origin(hwnd)
    s = dpi_scale(hwnd)
    return ox + int(round(x * s)), oy + int(round(y * s))


def focus(hwnd):
    if user32.IsIconic(hwnd):
        user32.ShowWindow(hwnd, SW_RESTORE)
    user32.SetForegroundWindow(hwnd)
    time.sleep(0.35)


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


def cmd_shot(hwnd, out):
    from PIL import ImageGrab

    focus(hwnd)
    ox, oy = client_origin(hwnd)
    cw, ch = client_size(hwnd)
    img = ImageGrab.grab(bbox=(ox, oy, ox + cw, oy + ch), all_screens=True)
    img.save(out)
    print(f"saved {out} ({img.width}x{img.height})")


def cmd_move(hwnd, x, y):
    focus(hwnd)
    sx, sy = to_screen(hwnd, x, y)
    user32.SetCursorPos(sx, sy)
    time.sleep(0.05)


def cmd_click(hwnd, x, y):
    focus(hwnd)
    sx, sy = to_screen(hwnd, x, y)
    user32.SetCursorPos(sx, sy)
    time.sleep(0.12)
    user32.mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0)
    time.sleep(0.06)
    user32.mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0)
    time.sleep(0.12)


def cmd_drag(hwnd, x1, y1, x2, y2, steps=24, hold=0.012):
    focus(hwnd)
    sx1, sy1 = to_screen(hwnd, x1, y1)
    sx2, sy2 = to_screen(hwnd, x2, y2)
    user32.SetCursorPos(sx1, sy1)
    time.sleep(0.15)
    user32.mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0)
    time.sleep(0.10)
    for i in range(1, steps + 1):
        t = i / steps
        user32.SetCursorPos(int(sx1 + (sx2 - sx1) * t), int(sy1 + (sy2 - sy1) * t))
        time.sleep(hold)
    time.sleep(0.10)
    user32.mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0)
    time.sleep(0.20)
    print(f"dragged ({x1},{y1}) -> ({x2},{y2})")


def main(argv):
    if len(argv) < 2:
        raise SystemExit(__doc__)
    cmd = argv[1]
    hwnd = find_window()
    if cmd == "info":
        cmd_info(hwnd)
    elif cmd == "shot":
        cmd_shot(hwnd, argv[2])
    elif cmd == "move":
        cmd_move(hwnd, int(argv[2]), int(argv[3]))
    elif cmd == "click":
        cmd_click(hwnd, int(argv[2]), int(argv[3]))
    elif cmd == "drag":
        cmd_drag(hwnd, *(int(v) for v in argv[2:6]))
    elif cmd == "place":
        # place <client_w> <client_h> <screen_x> <screen_y>
        cw, ch, sx, sy = (int(v) for v in argv[2:6])
        user32.MoveWindow(hwnd, sx, sy, cw, ch, True)
        time.sleep(0.4)
        cmd_info(hwnd)
    elif cmd == "maximize":
        focus(hwnd)
        user32.ShowWindow(hwnd, SW_MAXIMIZE)
        time.sleep(0.5)
        cmd_info(hwnd)
    elif cmd == "type":
        cmd_type(hwnd, argv[2])
    elif cmd == "key":
        cmd_key(hwnd, argv[2].lower())
    else:
        raise SystemExit(f"unknown command {cmd!r}\n{__doc__}")


if __name__ == "__main__":
    main(sys.argv)
