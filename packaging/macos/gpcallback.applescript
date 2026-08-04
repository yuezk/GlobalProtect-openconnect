-- URL scheme handler for `globalprotectcallback:` on macOS.
--
-- The SAML flow ends with the browser being redirected to
--   globalprotectcallback:<base64 auth data>
-- On Linux and BSD that scheme is claimed by gpgui.desktop via
-- `MimeType=x-scheme-handler/globalprotectcallback`, which runs
-- `gpclient launch-gui <url>`; that reads $TMPDIR/gpcallback.port and writes the
-- token to the socket gpauth's wait_auth_data() is listening on.
--
-- macOS has no equivalent registration, and a URL scheme can only be claimed by
-- an application bundle, so `gpclient connect --browser` waits forever for a
-- token that is never delivered. This applet supplies the missing hop.
--
-- Notes:
--  * TMPDIR is resolved with getconf rather than inherited. `do shell script`
--    runs with a minimal environment, and when TMPDIR is unset Rust's
--    std::env::temp_dir() falls back to /tmp, where the port file is not.
--  * Output goes to $TMPDIR/gpcallback.log, the same file wait_auth_data()
--    points the user at when the handoff hangs.
--  * gpclient is looked up in both Homebrew prefixes to cover Apple Silicon and
--    Intel, matching the paths in crates/common/src/constants.rs.

on open location this_URL
	set shellCmd to "T=$(getconf DARWIN_USER_TEMP_DIR); export TMPDIR=\"$T\"; LOG=\"$T/gpcallback.log\"; " & ¬
		"GPCLIENT=''; for p in /opt/homebrew/bin/gpclient /usr/local/bin/gpclient; do " & ¬
		"if [ -x \"$p\" ]; then GPCLIENT=\"$p\"; break; fi; done; " & ¬
		"if [ -z \"$GPCLIENT\" ]; then echo \"$(date): gpclient not found in /opt/homebrew/bin or /usr/local/bin\" >>\"$LOG\"; exit 1; fi; " & ¬
		"\"$GPCLIENT\" launch-gui " & quoted form of this_URL & " >>\"$LOG\" 2>&1"

	try
		do shell script shellCmd
	on error errMsg number errNum
		-- Never raise a modal dialog in the middle of a login; log and move on.
		set logLine to (do shell script "date") & ": gpcallback handler error " & errNum & ": " & errMsg
		do shell script "echo " & quoted form of logLine & " >> \"$(getconf DARWIN_USER_TEMP_DIR)/gpcallback.log\""
	end try
end open location

-- Opened without a URL (double-clicked, or launched by LaunchServices during
-- registration): do nothing rather than fail.
on run
end run
