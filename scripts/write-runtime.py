import os, pathlib, shlex
root=pathlib.Path(__file__).resolve().parents[1]
keys=['LD_LIBRARY_PATH','LIBGL_DRIVERS_PATH','__EGL_VENDOR_LIBRARY_FILENAMES','VK_DRIVER_FILES']
(root/'target/run-env.sh').write_text('\n'.join('export '+k+'='+shlex.quote(os.environ[k]) for k in keys if k in os.environ)+'\n')
