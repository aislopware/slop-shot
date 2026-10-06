#!/usr/bin/env python3
"""org.freedesktop.Notifications for the headless session, standing in for GNOME Shell's
banners (a headless shell does not own the name). Writes each notification as a JSON
line to argv[1]; tests press an action button through the extra Invoke method."""

import json
import sys

import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

IFACE = "org.freedesktop.Notifications"


class Server(dbus.service.Object):
    def __init__(self, bus, log_path):
        super().__init__(bus, "/org/freedesktop/Notifications")
        self.log_path = log_path
        self.next_id = 1

    @dbus.service.method(IFACE, in_signature="susssasa{sv}i", out_signature="u")
    def Notify(self, app_name, replaces_id, icon, summary, body, actions, hints, timeout):
        nid = int(replaces_id) or self.next_id
        self.next_id = max(self.next_id, nid) + 1
        record = {"id": nid, "app": str(app_name), "summary": str(summary), "body": str(body),
                  "actions": [str(a) for a in actions]}
        with open(self.log_path, "a") as f:
            f.write(json.dumps(record) + "\n")
        return dbus.UInt32(nid)

    @dbus.service.method(IFACE, in_signature="", out_signature="as")
    def GetCapabilities(self):
        return ["actions", "body"]

    @dbus.service.method(IFACE, in_signature="", out_signature="ssss")
    def GetServerInformation(self):
        return ("slopshot-e2e", "SlopShot", "1", "1.2")

    @dbus.service.method(IFACE, in_signature="u", out_signature="")
    def CloseNotification(self, nid):
        self.NotificationClosed(nid, 3)

    @dbus.service.method(IFACE, in_signature="us", out_signature="")
    def Invoke(self, nid, action_key):
        self.ActionInvoked(nid, action_key)
        self.NotificationClosed(nid, 2)

    @dbus.service.signal(IFACE, signature="us")
    def ActionInvoked(self, nid, action_key):
        pass

    @dbus.service.signal(IFACE, signature="uu")
    def NotificationClosed(self, nid, reason):
        pass


def main():
    DBusGMainLoop(set_as_default=True)
    bus = dbus.SessionBus()
    name = dbus.service.BusName(IFACE, bus, do_not_queue=True)  # noqa: F841 keeps the name
    Server(bus, sys.argv[1])
    print("ready", flush=True)
    GLib.MainLoop().run()


if __name__ == "__main__":
    main()
