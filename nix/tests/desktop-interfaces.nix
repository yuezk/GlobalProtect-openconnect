{ pkgs, package, module }:
let
  testPython = pkgs.python3.withPackages (ps: [ ps.pyatspi ]);
  typelibs = pkgs.lib.makeSearchPath "lib/girepository-1.0" [ pkgs.at-spi2-core pkgs.glib ];
  chooseTauri = pkgs.writeScript "choose-tauri" ''
    #!${testPython}/bin/python3
    import pyatspi
    import subprocess

    desktop = pyatspi.Registry.getDesktop(0)
    target = pyatspi.findDescendant(desktop, lambda node: node.name == "Interface: Tauri")
    if target is None:
        raise SystemExit("The Tauri control is not visible yet")
    # GTK4 exposes bounds relative to the window, even on X11.
    rect = target.queryComponent().getExtents(pyatspi.WINDOW_COORDS)
    if rect.width <= 0 or rect.height <= 0:
        raise SystemExit("The Tauri control has no visible bounds")
    window = subprocess.check_output(["xdotool", "search", "--onlyvisible", "--name",
                                      "^GP Connect Settings$"], text=True).splitlines()[-1]
    subprocess.run(["xdotool", "mousemove", "--window", window, str(rect.x + rect.width // 2),
                    str(rect.y + rect.height // 2), "click", "1"], check=True)
  '';
in
pkgs.testers.runNixOSTest {
  name = "gp-offline-desktop-interfaces";
  nodes.machine = {
    imports = [ module (pkgs.path + "/nixos/tests/common/x11.nix") ];
    programs.globalprotect-openconnect = { enable = true; inherit package; };
    test-support.displayManager.auto.user = "alice";
    users.users.alice = { isNormalUser = true; uid = 1000; };
    services.gnome.at-spi2-core.enable = true;
    environment.systemPackages = [ pkgs.xdotool ];
    security.polkit.extraConfig = ''
      polkit.addRule(function(action, subject) {
        if (subject.user == "alice" && action.id == "com.yuezk.gpgui.service") {
          return polkit.Result.YES;
        }
      });
    '';
    virtualisation.memorySize = 4096;
  };

  testScript = ''
    import shlex

    machine.start()
    machine.wait_for_unit("display-manager.service")
    def as_alice(command):
        environment = "export DISPLAY=:0 XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus GTK_A11Y=atspi; "
        return "su - alice -c " + shlex.quote(environment + command)

    machine.wait_until_succeeds(as_alice("xdotool getdisplaygeometry"))
    # Leave loopback available for the local privileged service.
    machine.succeed("for dev in /sys/class/net/*; do dev=$(basename \"$dev\"); [ \"$dev\" = lo ] || ip link set \"$dev\" down; done")

    def launch():
        machine.succeed(as_alice("gpclient launch-gui > ~/gp-launch.log 2>&1 & echo $! > ~/gp-launch.pid"))

    def wait_for_gui(name):
        # Identify the process which owns the mapped main window, including Nix's executable wrapper.
        machine.wait_until_succeeds(as_alice(
            "pid=$(xdotool search --onlyvisible --name '^GP Connect$' | tail -n 1 | xargs xdotool getwindowpid) && "
            "readlink /proc/$pid/exe | grep -E '/[.]?" + name + "(-wrapped)?$'"
        ))

    launch()
    wait_for_gui("gpgui-nsh")
    machine.succeed(as_alice("test ! -e ~/.config/gpclient/gui"))
    machine.succeed(as_alice("xdotool search --onlyvisible --name '^GP Connect$' windowactivate --sync key ctrl+comma"))
    machine.wait_until_succeeds(as_alice("xdotool search --onlyvisible --name '^GP Connect Settings$'"))
    machine.screenshot("native-shell-settings")
    machine.wait_until_succeeds(as_alice("GI_TYPELIB_PATH=${typelibs} ${chooseTauri}"))
    wait_for_gui("gpgui")
    machine.succeed(as_alice("grep -Fx gpgui ~/.config/gpclient/gui"))
    machine.succeed(as_alice("kill -TERM $(cat ~/gp-launch.pid)"))
    machine.wait_until_succeeds("test ! -e /var/run/gpservice.lock")

    launch()
    wait_for_gui("gpgui")
    machine.screenshot("tauri-relaunch")
  '';
}
