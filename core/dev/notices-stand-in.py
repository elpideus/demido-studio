"""A desktop's notification server, standing in for a real one on a private D-Bus (the freedesktop Desktop
Notifications Specification: org.freedesktop.Notifications). Writes down every notice it is given, and clicks
each one after two seconds, as a person would (ActionInvoked with "default"). For core/dev/desktop-trial.sh.
Usage: python3 notices-stand-in.py <log>"""
import sys
import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

LOG = sys.argv[1]


def note(text):
    with open(LOG, "a") as f:
        f.write(text + "\n")


class Notices(dbus.service.Object):
    def __init__(self, bus):
        super().__init__(bus, "/org/freedesktop/Notifications")
        self.last = 0

    @dbus.service.method("org.freedesktop.Notifications", in_signature="", out_signature="as")
    def GetCapabilities(self):
        return ["body", "actions"]

    @dbus.service.method("org.freedesktop.Notifications", in_signature="", out_signature="ssss")
    def GetServerInformation(self):
        return ("stand-in", "demido", "1", "1.2")

    @dbus.service.method("org.freedesktop.Notifications", in_signature="susssasa{sv}i", out_signature="u")
    def Notify(self, app, replaces, icon, summary, body, actions, hints, timeout):
        self.last += 1
        sid = self.last
        note(f"notice {sid}: app={app!s} title={summary!s} body={body!s} actions={list(map(str, actions))}")
        if "default" in [str(a) for a in actions]:
            GLib.timeout_add(2000, self.click, sid)
        return sid

    def click(self, sid):
        note(f"clicked {sid}")
        self.ActionInvoked(dbus.UInt32(sid), "default")
        return False

    @dbus.service.method("org.freedesktop.Notifications", in_signature="u", out_signature="")
    def CloseNotification(self, sid):
        note(f"closed {sid}")

    @dbus.service.signal("org.freedesktop.Notifications", signature="us")
    def ActionInvoked(self, sid, action):
        pass

    @dbus.service.signal("org.freedesktop.Notifications", signature="uu")
    def NotificationClosed(self, sid, reason):
        pass


DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus()
name = dbus.service.BusName("org.freedesktop.Notifications", bus)
Notices(bus)
note("listening")
GLib.MainLoop().run()
