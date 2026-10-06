#!/usr/bin/env python3
"""A full-screen window scrolling a tall image, the page the scrolling-capture test reads.

    python3 tests/e2e/page.py IMAGE.png
"""

import sys

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import Gtk  # noqa: E402


def activate(app):
    picture = Gtk.Picture.new_for_filename(sys.argv[1])
    picture.set_can_shrink(False)
    scroller = Gtk.ScrolledWindow(child=picture)
    scroller.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.ALWAYS)
    # No kinetic or animated scrolling: each wheel click moves the page right away.
    scroller.set_kinetic_scrolling(False)
    window = Gtk.ApplicationWindow(application=app, child=scroller)
    window.fullscreen()
    window.present()
    print("ready", flush=True)


app = Gtk.Application(application_id="com.thanglb.slopshot.E2EPage")
app.connect("activate", activate)
app.run([])
