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
  let route_detection = VPNC_SCRIPT
    .split_once("# =========== route handling ====================================")
    .unwrap()
    .1
    .split_once("if [ -n \"$IPROUTE\" ]; then")
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
CISCO_SPLIT_INC=1
CISCO_SPLIT_INC_0_ADDR=10.0.0.0
CISCO_SPLIT_INC_0_MASK=255.0.0.0
CISCO_SPLIT_INC_0_MASKLEN=8
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
                if [ -f "$state_dir/physical-ipv4" ]; then
                    dictionary=$(cat "$state_dir/physical-ipv4")
                else
                    echo '  No such key'
                fi
                ;;
            'get State:/Network/Service/ethernet-service/IPv4')
                dictionary=$(cat "$state_dir/ethernet-ipv4")
                ;;
            'remove State:/Network/Service/utun42/'*)
                if [ "$scutil_failure" = remove ] || [ "$scutil_failure" = "remove-${{command##*/}}" ]; then
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
{route_detection}
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
fn included_default_route_registers_tunnel_ipv4_for_dns() {
  for default_index in [0, 1] {
    assert_success(run_dns_test(&format!(
      r#"
CISCO_SPLIT_INC=2
CISCO_SPLIT_INC_0_ADDR=10.0.0.0
CISCO_SPLIT_INC_1_ADDR=192.168.50.0
CISCO_SPLIT_INC_{default_index}_ADDR=0.0.0.0
CISCO_SPLIT_INC_{default_index}_MASK=0.0.0.0
CISCO_SPLIT_INC_{default_index}_MASKLEN=0
configure_macos_dns || exit 1
grep -Fx 'd.add InterfaceName utun42' "$state_dir/IPv4" || exit 2
grep -Fx 'd.add Router 10.1.2.3' "$state_dir/IPv4" || exit 3
grep -Fx 'd.add Addresses * 10.1.2.3' "$state_dir/IPv4" || exit 4
grep -q GPConnectSourceService "$state_dir/IPv4" && exit 5
cmp "$state_dir/physical-dns" "$state_dir/original-physical-dns" || exit 6
"#,
    )));
  }
}

#[test]
fn included_subnet_routes_preserve_physical_ipv4_for_dns() {
  assert_success(run_dns_test(
    r#"
CISCO_SPLIT_INC=1
CISCO_SPLIT_INC_0_ADDR=10.0.0.0
CISCO_SPLIT_INC_0_MASK=255.0.0.0
CISCO_SPLIT_INC_0_MASKLEN=8
configure_macos_dns || exit 1
grep -Fx 'd.add InterfaceName en0' "$state_dir/IPv4" || exit 2
grep -Fx 'd.add Router 192.168.1.1' "$state_dir/IPv4" || exit 3
grep -Fx 'd.add GPConnectSourceService physical-service' "$state_dir/IPv4" || exit 4
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

#[test]
fn refreshes_dns_registration_from_a_new_primary_service() {
  assert_success(run_dns_test(
    r#"
configure_macos_dns || exit 1
printf '%s\n' 'd.add Addresses * 192.168.2.100' 'd.add SubnetMasks * 255.255.255.0' \
    'd.add InterfaceName en1' 'd.add Router 192.168.2.1' > "$state_dir/ethernet-ipv4"
printf 'ethernet-service\n' > "$state_dir/primary-service"
configure_macos_dns || exit 2
grep -Fx 'd.add InterfaceName en1' "$state_dir/IPv4" || exit 3
grep -Fx 'd.add Router 192.168.2.1' "$state_dir/IPv4" || exit 4
grep -Fx 'd.add GPConnectSourceService ethernet-service' "$state_dir/IPv4" || exit 5
if grep -Eq 'physical-service|192.168.1.1|InterfaceName en0' "$state_dir/IPv4"; then exit 6; fi
"#,
  ));
}

#[test]
fn missing_primary_service_or_source_dictionary_fails_configuration() {
  for setup in [
    "printf '' > \"$state_dir/primary-service\"",
    "rm \"$state_dir/physical-ipv4\"",
  ] {
    let output = run_dns_test(&format!("{setup}\nconfigure_macos_dns"));
    assert!(!output.status.success());
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(errors.contains("Cannot identify") || errors.contains("macOS network configuration failed"));
  }
}

#[test]
fn handles_absent_mixed_and_replaced_dns_servers() {
  assert_success(run_dns_test(
    r#"
INTERNAL_IP4_DNS=''
configure_macos_dns || exit 1
[ ! -f "$state_dir/DNS" ] && [ ! -f "$state_dir/IPv4" ] || exit 2
# The native producer exports both address families through INTERNAL_IP4_DNS.
INTERNAL_IP4_DNS='10.0.0.53 2001:db8::53'
configure_macos_dns || exit 3
grep -Fx 'd.add ServerAddresses * 10.0.0.53 2001:db8::53' "$state_dir/DNS" || exit 4
INTERNAL_IP4_DNS=2001:db8::54
configure_macos_dns || exit 5
grep -Fx 'd.add ServerAddresses * 2001:db8::54' "$state_dir/DNS" || exit 6
grep -Eq '10.0.0.53|2001:db8::53' "$state_dir/DNS" && exit 7
cmp "$state_dir/physical-dns" "$state_dir/original-physical-dns" || exit 8
"#,
  ));
}

#[test]
fn cleanup_attempts_the_other_key_when_one_removal_fails() {
  for failed_key in ["DNS", "IPv4"] {
    let removed_key = if failed_key == "DNS" { "IPv4" } else { "DNS" };
    let output = run_dns_test(&format!(
      r#"
configure_macos_dns || exit 1
scutil_failure=remove-{failed_key}
remove_macos_dns
[ "$?" -ne 0 ] || exit 2
[ -f "$state_dir/{failed_key}" ] || exit 3
[ ! -f "$state_dir/{removed_key}" ] || exit 4
"#
    ));
    assert!(output.status.success());
    assert_eq!(
      String::from_utf8_lossy(&output.stderr)
        .matches("macOS network configuration failed")
        .count(),
      1
    );
  }
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
set_default_route() {{ echo add-default4 >> "$state_dir/routes"; }}
set_network_route() {{ echo "add4 $1/$3" >> "$state_dir/routes"; }}
del_network_route() {{ echo "remove4 $1/$3" >> "$state_dir/routes"; }}
reset_default_route() {{ echo remove-default4 >> "$state_dir/routes"; }}
set_ipv6_default_route() {{ echo add-default6 >> "$state_dir/routes"; }}
reset_ipv6_default_route() {{ echo remove-default6 >> "$state_dir/routes"; }}
set_ipv6_network_route() {{ echo "add6 $1/$2" >> "$state_dir/routes"; }}
del_ipv6_network_route() {{ echo "remove6 $1/$2" >> "$state_dir/routes"; }}
ifconfig() {{ printf '%s\n' "$*" >> "$state_dir/interface-operations"; }}
ifconfig_syntax_del() {{ printf '%s\n' "$1"; }}
do_connect() {{{lifecycle}
"#,
    prefix = dns_script(),
  )
}

#[test]
fn ipv4_prefix_controls_setup_cleanup_and_dns_registration() {
  let script = lifecycle_script();
  let (definitions, _) = script.split_once("#### Main").unwrap();
  for (prefix, mask) in [
    (0, "0.0.0.0"),
    (1, "128.0.0.0"),
    (8, "255.0.0.0"),
    (32, "255.255.255.255"),
  ] {
    let expected_routes = if prefix == 0 {
      "add-default4\nremove-default4"
    } else {
      &format!("add4 0.0.0.0/{prefix}\nremove4 0.0.0.0/{prefix}")
    };
    let interface = if prefix == 0 { "utun42" } else { "en0" };
    let script = format!(
      r#"
{definitions}
CISCO_SPLIT_INC=1
CISCO_SPLIT_INC_0_ADDR=0.0.0.0
CISCO_SPLIT_INC_0_MASK={mask}
CISCO_SPLIT_INC_0_MASKLEN={prefix}
do_connect || exit 1
grep -Fx 'd.add InterfaceName {interface}' "$state_dir/IPv4" || exit 2
do_disconnect || exit 3
printf '%s\n' '{expected_routes}' > "$state_dir/expected-routes"
diff -u "$state_dir/expected-routes" "$state_dir/routes" || exit 4
[ ! -f "$state_dir/DNS" ] && [ ! -f "$state_dir/IPv4" ] || exit 5
"#
    );
    assert_success(Command::new("/bin/sh").args(["-c", &script]).output().unwrap());
  }
}

#[test]
fn ipv6_prefix_controls_setup_and_cleanup_independently_of_ipv4_dns() {
  let script = lifecycle_script();
  let (definitions, _) = script.split_once("#### Main").unwrap();
  for address in ["::", "0:0:0:0:0:0:0:0"] {
    for prefix in [0, 1, 64, 128] {
      let expected_routes = if prefix == 0 {
        "add4 10.0.0.0/8\nadd-default6\nremove4 10.0.0.0/8\nremove-default6"
      } else {
        &format!("add4 10.0.0.0/8\nadd6 {address}/{prefix}\nremove4 10.0.0.0/8\nremove6 {address}/{prefix}")
      };
      let script = format!(
        r#"
{definitions}
CISCO_IPV6_SPLIT_INC=1
CISCO_IPV6_SPLIT_INC_0_ADDR={address}
CISCO_IPV6_SPLIT_INC_0_MASKLEN={prefix}
INTERNAL_IP6_ADDRESS=2001:db8::42
do_connect || exit 1
grep -Fx 'd.add InterfaceName en0' "$state_dir/IPv4" || exit 2
do_disconnect || exit 3
printf '%s\n' '{expected_routes}' > "$state_dir/expected-routes"
diff -u "$state_dir/expected-routes" "$state_dir/routes" || exit 4
"#
      );
      assert_success(Command::new("/bin/sh").args(["-c", &script]).output().unwrap());
    }
  }
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
