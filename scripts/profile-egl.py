"""Report the current Wayland compositor's GL renderer without creating a window.

Set WAYLAND_DISPLAY to the compositor socket. This creates only a surfaceless
EGL context; it does not launch an app, send input, or alter any sessions.
"""
import ctypes as c
w=c.CDLL('libwayland-client.so.0')
w.wl_display_connect.argtypes=[c.c_char_p];w.wl_display_connect.restype=c.c_void_p
w.wl_display_disconnect.argtypes=[c.c_void_p]
d=w.wl_display_connect(None)
assert d, 'Cannot connect private compositor'
e=c.CDLL('libEGL.so.1')
e.eglGetDisplay.argtypes=[c.c_void_p];e.eglGetDisplay.restype=c.c_void_p
e.eglInitialize.argtypes=[c.c_void_p,c.POINTER(c.c_int),c.POINTER(c.c_int)]
e.eglChooseConfig.argtypes=[c.c_void_p,c.POINTER(c.c_int),c.POINTER(c.c_void_p),c.c_int,c.POINTER(c.c_int)]
e.eglBindAPI.argtypes=[c.c_uint]
e.eglCreateContext.argtypes=[c.c_void_p,c.c_void_p,c.c_void_p,c.POINTER(c.c_int)];e.eglCreateContext.restype=c.c_void_p
e.eglMakeCurrent.argtypes=[c.c_void_p,c.c_void_p,c.c_void_p,c.c_void_p]
e.eglGetProcAddress.argtypes=[c.c_char_p];e.eglGetProcAddress.restype=c.c_void_p
e.eglDestroyContext.argtypes=[c.c_void_p,c.c_void_p];e.eglTerminate.argtypes=[c.c_void_p]
display=e.eglGetDisplay(d);a=c.c_int();b=c.c_int()
assert e.eglInitialize(display,c.byref(a),c.byref(b)), 'eglInitialize failed'
attrs=(c.c_int*5)(0x3033,4,0x3040,4,0x3038);conf=c.c_void_p();count=c.c_int()
assert e.eglChooseConfig(display,attrs,c.byref(conf),1,c.byref(count)) and count.value
assert e.eglBindAPI(0x30a0)
ctx=e.eglCreateContext(display,conf,None,(c.c_int*3)(0x3098,2,0x3038));assert ctx
assert e.eglMakeCurrent(display,None,None,ctx), 'surfaceless context failed'
g=c.CFUNCTYPE(c.c_char_p,c.c_uint)(e.eglGetProcAddress(b'glGetString'))
print('GL_RENDERER='+g(0x1f01).decode(),flush=True)
e.eglMakeCurrent(display,None,None,None);e.eglDestroyContext(display,ctx);e.eglTerminate(display);w.wl_display_disconnect(d)
