"""Inspect and close only the measurement process's X11 windows without changing desktop settings."""

import ctypes
import json
import sys

DisplayPointer = ctypes.c_void_p
Window = ctypes.c_ulong


class Attributes(ctypes.Structure):
    _fields_ = [
        ("x", ctypes.c_int), ("y", ctypes.c_int), ("width", ctypes.c_int),
        ("height", ctypes.c_int), ("border_width", ctypes.c_int), ("depth", ctypes.c_int),
        ("visual", ctypes.c_void_p), ("root", Window), ("window_class", ctypes.c_int),
        ("bit_gravity", ctypes.c_int), ("win_gravity", ctypes.c_int),
        ("backing_store", ctypes.c_int), ("backing_planes", ctypes.c_ulong),
        ("backing_pixel", ctypes.c_ulong), ("save_under", ctypes.c_int),
        ("colormap", Window), ("map_installed", ctypes.c_int), ("map_state", ctypes.c_int),
        ("all_event_masks", ctypes.c_long), ("your_event_mask", ctypes.c_long),
        ("do_not_propagate_mask", ctypes.c_long), ("override_redirect", ctypes.c_int),
        ("screen", ctypes.c_void_p),
    ]


class ClientMessage(ctypes.Structure):
    _fields_ = [
        ("type", ctypes.c_int), ("serial", ctypes.c_ulong), ("send_event", ctypes.c_int),
        ("display", DisplayPointer), ("window", Window), ("message_type", Window),
        ("format", ctypes.c_int), ("data", ctypes.c_long * 5),
    ]


class Event(ctypes.Union):
    _fields_ = [("message", ClientMessage), ("pad", ctypes.c_long * 24)]


class Connection:
    def __init__(self, process_id):
        self.process_id = process_id
        self.xlib = ctypes.CDLL("libX11.so.6")
        self.bind_functions()
        self.display = self.xlib.XOpenDisplay(None)
        if not self.display:
            raise RuntimeError("X11 display is unavailable")
        self.root = self.xlib.XDefaultRootWindow(self.display)

    def bind_functions(self):
        pointer = ctypes.POINTER
        signatures = [
            ("XOpenDisplay", [ctypes.c_char_p], DisplayPointer),
            ("XCloseDisplay", [DisplayPointer], ctypes.c_int),
            ("XDefaultRootWindow", [DisplayPointer], Window),
            ("XInternAtom", [DisplayPointer, ctypes.c_char_p, ctypes.c_int], Window),
            ("XFree", [ctypes.c_void_p], ctypes.c_int),
            ("XFlush", [DisplayPointer], ctypes.c_int),
            ("XFetchName", [DisplayPointer, Window, pointer(ctypes.c_void_p)], ctypes.c_int),
            ("XGetInputFocus", [DisplayPointer, pointer(Window), pointer(ctypes.c_int)], ctypes.c_int),
            ("XGetWindowAttributes", [DisplayPointer, Window, pointer(Attributes)], ctypes.c_int),
            ("XSendEvent", [DisplayPointer, Window, ctypes.c_int, ctypes.c_long, pointer(Event)], ctypes.c_int),
            ("XQueryTree", [DisplayPointer, Window, pointer(Window), pointer(Window), pointer(pointer(Window)), pointer(ctypes.c_uint)], ctypes.c_int),
            ("XGetWindowProperty", [DisplayPointer, Window, Window, ctypes.c_long, ctypes.c_long,
                                    ctypes.c_int, Window, pointer(Window), pointer(ctypes.c_int),
                                    pointer(Window), pointer(Window), pointer(ctypes.c_void_p)], ctypes.c_int),
        ]
        for name, arguments, result in signatures:
            function = getattr(self.xlib, name)
            function.argtypes, function.restype = arguments, result

    def atom(self, name):
        return self.xlib.XInternAtom(self.display, name.encode(), 0)

    def values(self, window, name, expected_type):
        kind, form, count, remaining = Window(), ctypes.c_int(), Window(), Window()
        data = ctypes.c_void_p()
        status = self.xlib.XGetWindowProperty(
            self.display, window, self.atom(name), 0, 1024, 0, expected_type,
            ctypes.byref(kind), ctypes.byref(form), ctypes.byref(count),
            ctypes.byref(remaining), ctypes.byref(data),
        )
        try:
            if status or remaining.value:
                raise RuntimeError("Cannot read complete X11 window property")
            if not data or form.value == 0:
                return []
            if form.value != 32 or kind.value != expected_type:
                raise RuntimeError("Unexpected X11 property type")
            return [ctypes.cast(data, ctypes.POINTER(Window))[index] for index in range(count.value)]
        finally:
            if data:
                self.xlib.XFree(data)

    def tree(self, window):
        root, parent = Window(), Window()
        children, count = ctypes.POINTER(Window)(), ctypes.c_uint()
        if not self.xlib.XQueryTree(self.display, window, ctypes.byref(root), ctypes.byref(parent),
                                    ctypes.byref(children), ctypes.byref(count)):
            raise RuntimeError("Cannot read X11 window tree")
        try:
            return parent.value, [children[index] for index in range(count.value)]
        finally:
            if children:
                self.xlib.XFree(children)

    def belongs_to_process(self, window):
        return self.values(window, "_NET_WM_PID", 6) == [self.process_id]

    def focused(self):
        window, revert = Window(), ctypes.c_int()
        self.xlib.XGetInputFocus(self.display, ctypes.byref(window), ctypes.byref(revert))
        focused = window.value
        for _depth in range(32):
            if focused <= 1 or focused == self.root:
                return False
            if self.belongs_to_process(focused):
                return True
            focused, _children = self.tree(focused)
        return False

    def windows(self):
        _parent, children = self.tree(self.root)
        result = []
        for window in children:
            if not self.belongs_to_process(window):
                continue
            title = ctypes.c_void_p()
            self.xlib.XFetchName(self.display, window, ctypes.byref(title))
            try:
                name = ctypes.string_at(title).decode(errors="replace") if title else ""
                result.append({"id": window, "title": name})
            finally:
                if title:
                    self.xlib.XFree(title)
        return {"windows": result}

    def state(self, window):
        attributes = Attributes()
        if not self.xlib.XGetWindowAttributes(self.display, window, ctypes.byref(attributes)):
            raise RuntimeError("Cannot read X11 map state")
        return {"focused": self.focused(), "mapped": attributes.map_state == 2,
                "hidden": self.atom("_NET_WM_STATE_HIDDEN") in self.values(window, "_NET_WM_STATE", 4)}

    def close_window(self, window):
        # A standard close message never forwards page keystrokes through XIM.
        event = Event()
        event.message.type = 33
        event.message.display = self.display
        event.message.window = window
        event.message.message_type = self.atom("WM_PROTOCOLS")
        event.message.format = 32
        event.message.data[0] = self.atom("WM_DELETE_WINDOW")
        if not self.xlib.XSendEvent(self.display, window, 0, 0, ctypes.byref(event)):
            raise RuntimeError("X11 window close failed")
        self.xlib.XFlush(self.display)


def main():
    connection = Connection(int(sys.argv[1]))
    try:
        action = sys.argv[2]
        if action == "list":
            print(json.dumps(connection.windows()))
            return
        # Reject mutations or state queries unless the root window names this process.
        window = int(sys.argv[3])
        if not connection.belongs_to_process(window):
            raise RuntimeError("Window is not the measurement process")
        if action == "state":
            print(json.dumps(connection.state(window)))
            return
        if action == "close":
            connection.close_window(window)
            return
        raise RuntimeError("Unknown measurement action")
    finally:
        connection.xlib.XCloseDisplay(connection.display)


if __name__ == "__main__":
    main()
