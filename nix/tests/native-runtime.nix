{ pkgs, package }:
let
  guiProbe = pkgs.writeScript "gpgui-runtime-probe" ''
    #!${pkgs.python3}/bin/python3
    import os
    import pathlib
    import sys

    if "--version" in sys.argv:
        print("gpgui ${package.version}")
        sys.exit(0)

    assert "--service-credential-on-stdin" in sys.argv
    assert os.geteuid() == 1000
    assert "NoNewPrivs:\t0" in pathlib.Path("/proc/self/status").read_text()
    for name in ("GP_CLIENT_BINARY", "GP_SERVICE_BINARY", "GP_AUTH_BINARY",
                 "GP_VPNC_SCRIPT_INSTALLER_BINARY", "GP_HIP_SCRIPT_INSTALLER_BINARY",
                 "GP_VPNC_SCRIPT"):
        assert os.access(os.environ[name], os.X_OK), name
    header = sys.stdin.buffer.read(2)
    assert len(header) == 2
    length = int.from_bytes(header, "big")
    assert 0 < length <= 510
    assert len(sys.stdin.buffer.read(length)) == length
    pathlib.Path("/tmp/gp-gui-ready").write_text(str(os.getpid()))
    sys.stdin.buffer.read()
  '';
  testPackage = package.overrideAttrs (old: {
    postInstall = (old.postInstall or "") + ''
      install -m755 ${guiProbe} $out/bin/gpgui
    '';
  });
  hostCollector = pkgs.writeShellScriptBin "clamscan" ''
    echo 'ClamAV 1.4.0/27123/Mon Oct 7 00:00:00 2026'
  '';
in
pkgs.testers.runNixOSTest {
  name = "gp-native-runtime-${package.pname}";
  nodes.machine = {
    environment.systemPackages = [ testPackage hostCollector ];
    users.users.alice = { isNormalUser = true; uid = 1000; };
    security.polkit.enable = true;
    security.polkit.extraConfig = ''
      polkit.addRule(function(action, subject) {
        if (subject.user == "alice" && (
          action.id == "com.yuezk.gpgui.service" ||
          action.id == "com.yuezk.gpgui.manage-vpnc-script" ||
          action.id == "com.yuezk.gpgui.manage-root-hip-script"
        )) return polkit.Result.YES;
      });
    '';
    virtualisation.memorySize = 4096;
  };
  testScript = ''
    import base64
    import json
    import shlex

    machine.start()
    machine.wait_for_unit("polkit.service")
    package = "${testPackage}"

    def as_alice(command):
        return machine.succeed("su - alice -c " + shlex.quote(command))

    # Exercise real host pkexec, private credential pipes and launcher cancellation.
    command = package + "/bin/gpclient --lock-file /tmp/gp-client.lock launch-gui"
    as_alice(command + " > /tmp/gp-client-output 2>&1 & echo $! > /tmp/gp-client-pid")
    machine.wait_until_succeeds("test -s /tmp/gp-gui-ready")
    service_pid = machine.succeed("cut -d: -f1 /var/run/gpservice.lock").strip()
    machine.succeed("test $(stat -c %u /proc/" + service_pid + ") = 0")
    as_alice("kill -TERM $(cat /tmp/gp-client-pid)")
    machine.wait_until_succeeds("test ! -e /proc/" + service_pid)
    machine.wait_until_succeeds("! kill -0 $(cat /tmp/gp-gui-ready)")

    # The optional collector lives in the NixOS system profile, outside the package's tool closure.
    report = machine.succeed(package + "/bin/gpclient hip --client-version 6.3.3-619 "
        "--client-os linux --cookie fixture --md5 0123456789abcdef0123456789abcdef")
    assert "ClamAV" in report, report

    script = base64.b64encode(b"#!/bin/sh\nexit 0\n").decode()
    hip_request = json.dumps({"originalPath": "/home/alice/hip.sh", "contentsBase64": script})
    hip_installer = package + "/libexec/gpclient/gp-hip-script-installer"
    approval = json.loads(as_alice("printf %s " + shlex.quote(hip_request)
        + " | pkexec " + hip_installer + " install"))
    assert approval["ownerUid"] == 1000
    as_alice("pkexec " + hip_installer + " revoke " + shlex.quote(approval["approvalId"]))

    vpn_request = json.dumps({"metadata": {"source": "file", "value": "/home/alice/vpnc.sh"},
                             "contents": script})
    vpn_installer = package + "/libexec/gpclient/gp-vpnc-script-installer"
    as_alice("printf %s " + shlex.quote(vpn_request) + " | pkexec " + vpn_installer + " install")
    machine.succeed("test -x /var/lib/gpclient/scripts/vpnc-script")
    as_alice("pkexec " + vpn_installer + " remove")
    machine.succeed("test ! -e /var/lib/gpclient/scripts/vpnc-script")
  '';
}
