use super::VPNC_SCRIPT;
use std::process::{Command, Output};

// Exercise the production DNS functions and lifecycle dispatch without running
// script initialization or any real network commands. The fake store persists
// across scutil subprocesses, just as the real dynamic store does.
fn dns_script() -> String {
  let selection_start = VPNC_SCRIPT
    .find("if [ \"$OS\" = \"Darwin\" ]; then\n\tMODIFYRESOLVCONF=")
    .unwrap();
  let selection = VPNC_SCRIPT[selection_start..]
    .split_once("# =========== tunnel interface handling")
    .unwrap()
    .0;
  let functions = VPNC_SCRIPT
    .split_once("# =========== macOS resolver configuration =========================")
    .unwrap()
    .1
    .split_once("# =========== resolv.conf handling for other OSes =========================")
    .unwrap()
    .0;
  format!(
    r#"
state_dir=$(mktemp -d) || exit 1
trap 'rm -rf "$state_dir"' EXIT
TUNDEV=utun42
OS=Darwin
INTERNAL_IP4_ADDRESS=10.1.2.3
INTERNAL_IP4_DNS='10.0.0.53 10.0.0.54'
CISCO_DEF_DOMAIN='corp.example.com internal.example.com'
CISCO_SPLIT_INC=0
printf 'physical-service\n' > "$state_dir/primary-service"
printf '%s\n' 'd.add Addresses * 192.168.1.100' 'd.add SubnetMasks * 255.255.255.0' \
    'd.add InterfaceName en0' 'd.add Router 192.168.1.1' > "$state_dir/physical-ipv4"
printf '192.168.1.1\n' > "$state_dir/physical-dns"
cp "$state_dir/physical-dns" "$state_dir/original-physical-dns"
networksetup() {{ echo 'Unexpected physical DNS modification' >&2; exit 99; }}
scutil() {{
    if [ "$scutil_failure" = process ]; then return 1; fi
    dictionary=''
    while IFS= read -r command; do
        case "$command" in
            '') ;;
            d.init) dictionary='' ;;
            'd.add '*) dictionary="$dictionary
$command" ;;
            'set State:/Network/Service/utun42/'*)
                if [ "$scutil_failure" = command ]; then
                    echo '  Permission denied'
                else
                    printf '%s\n' "$dictionary" > "$state_dir/${{command##*/}}"
                fi
                ;;
            'get State:/Network/Service/utun42/'*)
                if [ -f "$state_dir/${{command##*/}}" ]; then
                    dictionary=$(cat "$state_dir/${{command##*/}}")
                else
                    echo '  No such key'
                fi
                ;;
            'show State:/Network/Global/IPv4')
                printf '  PrimaryService : %s\n' "$(cat "$state_dir/primary-service")"
                ;;
            'show State:/Network/Service/utun42/IPv4')
                sed -n 's/^d.add GPConnectSourceService /  GPConnectSourceService : /p' "$state_dir/IPv4"
                ;;
            'get State:/Network/Service/physical-service/IPv4')
                dictionary=$(cat "$state_dir/physical-ipv4")
                ;;
            'remove State:/Network/Service/utun42/'*)
                if [ "$scutil_failure" = remove ]; then
                    echo '  Permission denied'
                else
                    rm "$state_dir/${{command##*/}}" || return 1
                fi
                ;;
            quit) ;;
            *) echo "Unexpected scutil command: $command" >&2; return 1 ;;
        esac
    done
}}
{selection}
{functions}
"#
  )
}

fn run_dns_test(body: &str) -> Output {
  let script = format!("{}\n{}", dns_script(), body);
  Command::new("/bin/sh").args(["-c", &script]).output().unwrap()
}

fn assert_success(output: Output) {
  assert!(
    output.status.success(),
    "stdout: {}\nstderr: {}",
    String::from_utf8_lossy(&output.stdout),
    String::from_utf8_lossy(&output.stderr)
  );
  assert!(output.stderr.is_empty(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn publishes_default_dns_and_search_suffixes_without_physical_dns_changes() {
  assert_success(run_dns_test(
    r#"
configure_macos_dns || exit 1
grep -Fx 'd.add ServerAddresses * 10.0.0.53 10.0.0.54' "$state_dir/DNS" || exit 2
grep -Fx 'd.add SearchDomains * corp.example.com internal.example.com' "$state_dir/DNS" || exit 3
grep -Fx 'd.add SearchOrder # 50000' "$state_dir/DNS" || exit 4
grep -Eq 'SupplementalMatchDomains|DomainName' "$state_dir/DNS" && exit 5
grep -Fx 'd.add OverridePrimary # 1' "$state_dir/IPv4" || exit 6
grep -Fx 'd.add Router 192.168.1.1' "$state_dir/IPv4" || exit 7
grep -Fx 'd.add InterfaceName en0' "$state_dir/IPv4" || exit 13
grep -Fx 'd.add GPConnectSourceService physical-service' "$state_dir/IPv4" || exit 14
grep -q '10.1.2.3' "$state_dir/IPv4" && exit 15
cmp "$state_dir/physical-dns" "$state_dir/original-physical-dns" || exit 8
cp "$state_dir/DNS" "$state_dir/first-dns"
# Once macOS elects our service, reconnect must read the original source,
# rather than clone the VPN-owned snapshot again.
printf 'utun42\n' > "$state_dir/primary-service"
sed -i '' 's/192.168.1.100/192.168.1.101/' "$state_dir/physical-ipv4"
configure_macos_dns || exit 9
cmp "$state_dir/DNS" "$state_dir/first-dns" || exit 10
grep -Fx 'd.add Addresses * 192.168.1.101' "$state_dir/IPv4" || exit 16
# Full-tunnel service registration retains its existing Router behavior.
unset CISCO_SPLIT_INC
configure_macos_dns || exit 11
grep -Fx 'd.add Router 10.1.2.3' "$state_dir/IPv4" || exit 12
"#,
  ));
}

#[test]
fn replaces_dns_and_removes_stale_suffixes_and_owned_state() {
  assert_success(run_dns_test(
    r#"
configure_macos_dns || exit 1
INTERNAL_IP4_DNS=10.0.0.55
CISCO_DEF_DOMAIN=''
configure_macos_dns || exit 2
grep -Fx 'd.add ServerAddresses * 10.0.0.55' "$state_dir/DNS" || exit 3
grep -Eq '10.0.0.53|SearchDomains|corp.example.com' "$state_dir/DNS" && exit 4
INTERNAL_IP4_DNS=''
configure_macos_dns || exit 5
[ ! -f "$state_dir/DNS" ] && [ ! -f "$state_dir/IPv4" ] || exit 6
# Removing missing owned state is idempotent, even without a DNS environment.
remove_macos_dns || exit 7
cmp "$state_dir/physical-dns" "$state_dir/original-physical-dns" || exit 8
"#,
  ));
}

#[test]
fn reports_scutil_command_and_process_failures_and_attempts_both_removals() {
  for failure in ["command", "process"] {
    let output = run_dns_test(&format!("scutil_failure={failure}\nconfigure_macos_dns"));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("macOS network configuration failed"));
  }
  let output = run_dns_test(
    r#"
configure_macos_dns || exit 1
scutil_failure=remove
remove_macos_dns
"#,
  );
  assert!(!output.status.success());
  let errors = String::from_utf8_lossy(&output.stderr);
  assert_eq!(errors.matches("macOS network configuration failed").count(), 2);
}

// Stub route/interface operations and use production lifecycle functions to
// check DNS dispatch, failure propagation, and interface cleanup after failure.
fn lifecycle_script() -> String {
  let lifecycle = VPNC_SCRIPT.split_once("do_connect() {").unwrap().1;
  format!(
    r#"
{prefix}
do_ifconfig() {{ :; }}
set_vpngateway_route() {{ :; }}
del_vpngateway_route() {{ :; }}
reset_default_route() {{ :; }}
ifconfig() {{ printf '%s\n' "$*" >> "$state_dir/interface-operations"; }}
ifconfig_syntax_del() {{ printf '%s\n' "$1"; }}
do_connect() {{{lifecycle}
"#,
    prefix = dns_script(),
  )
}

#[test]
fn lifecycle_reapplies_dns_on_reconnect_and_propagates_failures() {
  // Source just the production lifecycle definitions, then run its dispatch.
  let script = lifecycle_script();
  for reason in ["connect", "reconnect"] {
    let output = Command::new("/bin/sh")
      .args(["-c", &script])
      .env("reason", reason)
      .env("scutil_failure", "command")
      .output()
      .unwrap();
    assert!(!output.status.success(), "{reason} hid DNS failure");
    assert!(String::from_utf8_lossy(&output.stderr).contains("macOS network configuration failed"));
  }
  let (definitions, dispatch) = script.split_once("#### Main").unwrap();
  let dispatch = dispatch.trim_end().strip_suffix("exit 0").unwrap();
  for reason in ["connect", "reconnect"] {
    let script = format!(
      r#"
{definitions}
reason={reason}
{dispatch}
grep -Fx 'd.add ServerAddresses * 10.0.0.53 10.0.0.54' "$state_dir/DNS" || exit 1
"#
    );
    assert_success(Command::new("/bin/sh").args(["-c", &script]).output().unwrap());
  }
  let script = format!(
    r#"
{definitions}
configure_macos_dns || exit 1
INTERNAL_IP4_DNS=''
reason=disconnect
{dispatch}
[ ! -f "$state_dir/DNS" ] && [ ! -f "$state_dir/IPv4" ] || exit 2
grep -Fx 'utun42 down' "$state_dir/interface-operations" || exit 3
cmp "$state_dir/physical-dns" "$state_dir/original-physical-dns" || exit 4
"#
  );
  let output = Command::new("/bin/sh").args(["-c", &script]).output().unwrap();
  assert_success(output);
}

#[test]
fn disconnect_still_tears_down_interface_when_dns_cleanup_fails() {
  let script = lifecycle_script();
  let (definitions, _) = script.split_once("#### Main").unwrap();
  let script = format!(
    r#"
{definitions}
configure_macos_dns || exit 1
scutil_failure=remove
do_disconnect
[ "$?" -ne 0 ] || exit 2
grep -Fx 'utun42 down' "$state_dir/interface-operations" || exit 3
"#
  );
  let output = Command::new("/bin/sh").args(["-c", &script]).output().unwrap();
  assert!(output.status.success());
  assert_eq!(
    String::from_utf8_lossy(&output.stderr)
      .matches("macOS network configuration failed")
      .count(),
    2
  );
}
