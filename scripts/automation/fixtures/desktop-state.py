#!/usr/bin/env python3
"""Print the owner's desktop state as JSON: the physical pointer position
and the frontmost application. Reads only; needs no permissions.
macOS only (Quartz and lsappinfo)."""
import ctypes, ctypes.util, json, subprocess

class Point(ctypes.Structure):
    _fields_ = [("x", ctypes.c_double), ("y", ctypes.c_double)]

quartz = ctypes.CDLL(ctypes.util.find_library("ApplicationServices"))
quartz.CGEventCreate.restype = ctypes.c_void_p
quartz.CGEventCreate.argtypes = [ctypes.c_void_p]
quartz.CGEventGetLocation.restype = Point
quartz.CGEventGetLocation.argtypes = [ctypes.c_void_p]
event = quartz.CGEventCreate(None)
p = quartz.CGEventGetLocation(event)
front = subprocess.run(["lsappinfo", "front"], capture_output=True, text=True).stdout.strip()
info = subprocess.run(["lsappinfo", "info", "-only", "name", front], capture_output=True, text=True).stdout.strip()
print(json.dumps({"pointer": [round(p.x, 1), round(p.y, 1)], "front": info}))
