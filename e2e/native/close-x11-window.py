"""Asks an X11 window to close, as a window manager does when its close button is pressed.

Usage: close-x11-window.py PID TITLE

Sends WM_DELETE_WINDOW to every top-level window of process PID titled TITLE. GTK turns it
into the window's delete-event, the same path a person's click takes. Under Xvfb there is no
window manager to send it, and the standard library reaches libX11 through ctypes, so nothing
needs installing. Exits with 1 if no such window exists.
"""

import ctypes
import sys
from ctypes import POINTER, byref, c_char_p, c_int, c_long, c_uint, c_ulong, c_void_p

pid, title = int(sys.argv[1]), sys.argv[2].encode()

x11 = ctypes.CDLL("libX11.so.6")
x11.XOpenDisplay.restype = c_void_p
x11.XOpenDisplay.argtypes = [c_char_p]
x11.XDefaultRootWindow.restype = c_ulong
x11.XDefaultRootWindow.argtypes = [c_void_p]
x11.XInternAtom.restype = c_ulong
x11.XInternAtom.argtypes = [c_void_p, c_char_p, c_int]
x11.XQueryTree.argtypes = [
    c_void_p, c_ulong, POINTER(c_ulong), POINTER(c_ulong), POINTER(POINTER(c_ulong)), POINTER(c_uint)
]
x11.XGetWindowProperty.argtypes = [
    c_void_p, c_ulong, c_ulong, c_long, c_long, c_int, c_ulong,
    POINTER(c_ulong), POINTER(c_int), POINTER(c_ulong), POINTER(c_ulong), POINTER(c_void_p),
]
x11.XFetchName.argtypes = [c_void_p, c_ulong, POINTER(c_void_p)]
x11.XSendEvent.argtypes = [c_void_p, c_ulong, c_int, c_long, c_void_p]
x11.XFree.argtypes = [c_void_p]
x11.XFlush.argtypes = [c_void_p]
x11.XCloseDisplay.argtypes = [c_void_p]


class ClientMessage(ctypes.Structure):
    _fields_ = [
        ("type", c_int),
        ("serial", c_ulong),
        ("send_event", c_int),
        ("display", c_void_p),
        ("window", c_ulong),
        ("message_type", c_ulong),
        ("format", c_int),
        ("data", c_long * 5),
    ]


class XEvent(ctypes.Union):
    # Xlib pads every event to 24 longs.
    _fields_ = [("xclient", ClientMessage), ("pad", c_long * 24)]


display = x11.XOpenDisplay(None)
if not display:
    sys.exit("cannot open the X display")
cardinal = x11.XInternAtom(display, b"CARDINAL", 0)
wm_pid = x11.XInternAtom(display, b"_NET_WM_PID", 0)
wm_protocols = x11.XInternAtom(display, b"WM_PROTOCOLS", 0)
wm_delete = x11.XInternAtom(display, b"WM_DELETE_WINDOW", 0)


def pid_of(window):
    kind, size, count, after, data = c_ulong(), c_int(), c_ulong(), c_ulong(), c_void_p()
    status = x11.XGetWindowProperty(
        display, window, wm_pid, 0, 1, 0, cardinal,
        byref(kind), byref(size), byref(count), byref(after), byref(data),
    )
    if status != 0 or not data.value:
        return None
    # Xlib returns 32-bit properties as an array of C longs.
    value = ctypes.cast(data, POINTER(c_long))[0] if count.value else None
    x11.XFree(data)
    return value


def title_of(window):
    name = c_void_p()
    if not x11.XFetchName(display, window, byref(name)) or not name.value:
        return None
    value = ctypes.string_at(name.value)
    x11.XFree(name)
    return value


root_out, parent, children, count = c_ulong(), c_ulong(), POINTER(c_ulong)(), c_uint()
x11.XQueryTree(
    display, x11.XDefaultRootWindow(display), byref(root_out), byref(parent), byref(children), byref(count)
)
targets = [
    children[i]
    for i in range(count.value)
    if pid_of(children[i]) == pid and title_of(children[i]) == title
]
for window in targets:
    event = XEvent()
    event.xclient.type = 33  # ClientMessage
    event.xclient.window = window
    event.xclient.message_type = wm_protocols
    event.xclient.format = 32
    event.xclient.data[0] = wm_delete
    event.xclient.data[1] = 0  # CurrentTime
    x11.XSendEvent(display, window, 0, 0, byref(event))
x11.XFlush(display)
x11.XCloseDisplay(display)
if not targets:
    sys.exit(f"no window of process {pid} is titled {title.decode()}")
print(f"asked {len(targets)} window(s) to close")
