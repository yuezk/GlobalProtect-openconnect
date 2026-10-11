# WebKitGTK rendering investigation

## KDE redraw failure with either WebKit override

On October 11, 2026, local KDE tests reproduced GUI flickering and incomplete
redraws with either override alone and with both overrides enabled. All three
commands exhibited the problem:

```sh
WEBKIT_DISABLE_DMABUF_RENDERER=1 gpclient launch-gui -vv

WEBKIT_DISABLE_COMPOSITING_MODE=1 gpclient launch-gui -vv

WEBKIT_DISABLE_COMPOSITING_MODE=1 \
WEBKIT_DISABLE_DMABUF_RENDERER=1 \
gpclient launch-gui -vv
```

The window intermittently loses most of its content. Hovering a button or
focusing an input redraws that control while other regions remain missing.
The supplied screenshots show the desktop behind missing regions of the GUI,
with only some controls still visible. Earlier testing reported normal
rendering after removing the overrides.

Each override independently triggers the observed problem on the tested KDE
setup. The tests do not establish the underlying rendering defect or whether
other graphics configurations are affected. The exact application version,
KDE version, display protocol,
GPU/driver, and WebKitGTK version were not recorded with this test.

Do not recommend either override as a general GUI workaround. Close the app
completely between rendering tests so each launch uses the intended environment.
These command-scoped settings do not persist after the application exits.

## KDE test with NVIDIA explicit sync disabled

On the same KDE setup, the following command did not cause the flickering or
incomplete redraws observed with either WebKit override:

```sh
__NV_DISABLE_EXPLICIT_SYNC=1 gpclient launch-gui -vv
```

This establishes that the setting did not reproduce the redraw problem in this
local test. It does not establish whether it fixes the authentication-window
failure in #666 or whether the tested graphics driver uses this setting.

## Relationship to the authentication-window investigation

[Issue #666](https://github.com/yuezk/GlobalProtect-openconnect/issues/666)
reports a separate authentication-window failure on Hyprland with
`Error 71 (Protocol error) dispatching to Wayland display`. Whether that failure
shares a cause with this KDE redraw problem is not confirmed.

The automatic `WEBKIT_DISABLE_COMPOSITING_MODE=1` and
`WEBKIT_DISABLE_DMABUF_RENDERER=1` settings were removed in 3.0.0. Automatically
setting `LIBGL_ALWAYS_SOFTWARE=1` when missing on Wayland remains in place.
Restoring either WebKit override globally would risk reintroducing the observed
redraw failure.

[WebKit bug #280210, comment 14](https://bugs.webkit.org/show_bug.cgi?id=280210#c14)
also describes incomplete rendering with DMABUF disabled on an NVIDIA/KWin
setup. It reports `__NV_DISABLE_EXPLICIT_SYNC=1` as an alternative that retains
DMABUF rendering. Other comments report flickering with that alternative, so it
requires testing on the affected machine; it is not a validated general fix
for GP Connect. The local KDE test above found no redraw failure with this
alternative, but the reporter's crash still needs a separate test.

## Opaque GUI recovery verification

The local implementation selects `WEBKIT_DISABLE_DMABUF_RENDERER=1` only for a
single retry after a recognized Wayland GDK rendering crash. An explicit caller
setting prevents automatic selection. With that value exactly `1`, the GUI uses
opaque, square main/settings/update windows while retaining the saved corner
preference. It does not select `WEBKIT_DISABLE_COMPOSITING_MODE`.

On the Fedora 44 KDE Wayland VM (Plasma/KWin 6.6.4, WebKitGTK 2.54.1,
VMware vmwgfx), a simulated GDK crash followed by the real opaque GUI rendered
the main and settings windows fully during focus changes through both public
launch commands. This supports the opaque approach on this VM; it does not
prove the original NVIDIA crash is resolved or establish behavior on every
KDE setup. See [the recovery design](design/webkit-crash-recovery.md) for the
retry boundaries and remaining verification gaps. The earlier warning about
general overrides concerns the transparent-window behavior tested above.

The user also manually tested the recovered GUI on this VM and confirmed no
flickering.
