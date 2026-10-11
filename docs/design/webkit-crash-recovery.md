# WebKitGTK crash recovery

Status: implemented and independently reviewed; KDE VM verification recorded below.

## Outcome and evidence

When a bundled GTK/WebKit app terminates with the recognized Wayland protocol error, retry once with `WEBKIT_DISABLE_DMABUF_RENDERER=1`. GP Connect uses opaque windows in that mode so recovery does not introduce the KDE redraw failure. Normal launches keep their existing rendering behavior.

The KDE VM comparison used the same newly built binaries: transparent windows lost content with DMABUF disabled; opaque main and settings windows remained drawn during focus changes and reopening. The experiment changed native transparency, backing backgrounds, outer corner rounding and modal clipping together. It validates this combination on Fedora KDE/VMware, not the reporter's NVIDIA startup failure. Recorded versions and reproduction commands are in the workspace's `gp_logs/kde-opaque-test-2026-10-11/README.md`.

References: [GP issue #666](https://github.com/yuezk/GlobalProtect-openconnect/issues/666), [Tauri PR #16226](https://github.com/tauri-apps/tauri/pull/16226), and [rendering investigation](../webkit-rendering.md). The upstream PR proposes a warning, not automatic recovery. A protocol error does not prove DMABUF caused the failure; this is a narrowly selected recovery attempt.

## Scope and retry rules

Apply the policy on Linux, FreeBSD and OpenBSD builds using GTK/WebKit. macOS behavior is unchanged. Use the effective child environment after desktop-session restoration and privilege drop, rather than the parent shell environment, to determine eligibility. Nonempty WAYLAND_DISPLAY is required; XDG_SESSION_TYPE alone is insufficient. DISPLAY may coexist with WAYLAND_DISPLAY, so it does not disqualify recovery.

A retry requires all of these:

- The directly monitored bundled gpgui, gpgui-helper or embedded-mode gpauth process exited unsuccessfully (nonzero status or signal).
- Its stderr contained a GDK diagnostic with `Error 71 (Protocol error) dispatching to Wayland display`. Match the diagnostic line, not arbitrary occurrences in an application log, preflight warning or quoted suggestion.
- DMABUF override is absent in the effective child environment. Any explicit value, including `0`, empty or `1`, prevents automatic replacement.
- No cancellation, explicit termination or service shutdown is in progress, and the recovery budget has not been spent.

Do not retry ordinary authentication rejection, successful exit with malformed output, generic crashes, missing executables, version checks, ordinary download helper failures, external-browser auth, callback-handler commands, or a WebKit subprocess crash while the monitored parent survives. Do not use WebKit version, GPU vendor or desktop name as a trigger. No timeout-based detection of flicker or hangs.

For the desktop, allow at most one automatic rendering recovery during a DesktopLauncher lifetime, including the download helper and GUI-requested restarts. Program resolution and helper execution run inside the supervised attempt; a recognized helper crash requests a fresh service credential before resolving again. The helper and GUI share the same observed child runner, log destination, cancellation ownership and single retry budget. The helper already uses an opaque native window. Once selected, the recovered rendering environment remains in effect for later legitimate GUI restarts in that desktop session; nothing is persisted to disk. A second crash is reported and terminates that launch session.

For authentication, allow at most one rendering recovery in one top-level connection attempt, shared across portal and gateway authentication. Keep this budget and selected auth rendering mode in the owning connection workflow. Create a fresh budget at each serialized top-level run_connection_attempt, including existing startup/resume attempts; never reset within portal/gateway fallback or authentication loops. This feature does not schedule new connection attempts or add an automatic reconnect loop. Concurrent connections do not share mutable global recovery state. If the GUI itself already runs with DMABUF disabled, its auth child inherits that setting and has no additional override retry.

The retry preserves the original executable, identity, certificate/TLS options and environment except the DMABUF setting. Do not automatically set WEBKIT_DISABLE_COMPOSITING_MODE, __NV_DISABLE_EXPLICIT_SYNC, or change LIBGL_ALWAYS_SOFTWARE. Keep any user-supplied values unchanged.

## GUI entrypoints

Cover both public launch commands on Linux/FreeBSD/OpenBSD:

- `gpclient launch-gui` starts DesktopLauncher directly.
- A release-build `gpgui` without `--service-credential-on-stdin` delegates to `gpclient launch-gui` through gpgui's existing app::launch_gui function. This wrapper runs before App::new, runtime environment patching and webview initialization. It inherits the caller environment, forwards --minimized, replaces the bootstrap process with gpclient on Linux/FreeBSD/OpenBSD, so signals, cleanup and final exit status belong to the supervisor. macOS retains its existing launch behavior. Keep this handoff: the same DesktopLauncher supervises and retries the actual GUI child, so no second retry loop belongs in the gpgui wrapper.

DesktopLauncher launches the actual GUI with `--service-credential-on-stdin`; that child must not delegate back to gpclient. Test this guard explicitly to prevent recursion. Existing GUI activation via either public command must return without starting another service, GUI or recovery budget. Desktop-file launch entries that use either command receive the same recovery behavior. The current Linux desktop file uses /usr/bin/gpclient launch-gui %u and the BSD desktop file uses /usr/local/bin/gpclient launch-gui %u. Cover these shipped entrypoints; authentication callback URLs remain routed through the existing callback handler without GUI crash recovery.

Release-path verification must run `gpgui` and `gpgui --minimized`, in addition to `gpclient launch-gui`, with a synthetic recognized crash in the actual child. Confirm one service authorization/session owner, one recovery, fresh credentials, propagated rendering environment, cancellation cleanup and final failure status. Test manual DMABUF override through the public gpgui command too. The earlier VM experiment used debug binaries through gpclient and does not verify this release-only delegation branch.

Debug `gpgui` without stdin credentials intentionally uses the development bootstrap socket and runs directly, with no DesktopLauncher supervisor. Opaque appearance still applies when the override is supplied, but automatic desktop crash retry is a release-launch feature; do not add a separate self-restart loop for the development path. Direct internal launches with `--service-credential-on-stdin` require their existing supervising caller. macOS app-bundle launch remains unchanged.

## Process observation module

Put the classifier and bounded stderr observer under gpapi::process, alongside the current launchers. Keep policy independent of gpservice transport models. GUI and auth launchers use the same classifier; their owners control retry sequencing and budgets.

Capture and continuously drain stderr while the child runs. Forward the original bytes to the existing destination: inherited stderr for auth, append-only GP_LOG_FILE for GUI when supplied, otherwise inherited stderr. Do not route stderr through stdout: gpauth stdout is the bounded credential JSON channel. Do not duplicate child messages through the Rust logger. Emit the parent's retry/final-failure records through the existing logger and selected log format.

The observer retains only a fixed-size rolling diagnostic fragment and a match flag, not full stderr. It handles read chunks splitting a diagnostic and lines exceeding the bound without unbounded allocations. Matching does not require storing a username, SAML request or cookie. A stderr observer read/forwarding error is a launch error, not a crash classification. A GUI credential-write BrokenPipe can be a consequence of the early abort: retain that error while observing and reaping the child. If the unsuccessful exit and GDK diagnostic confirm the rendering failure, return that classification; otherwise return the original write error. Never treat BrokenPipe alone as a recovery trigger. Drain both stdout and stderr concurrently for auth to avoid pipe deadlocks; keep the current auth stdout size limit. Do not change generic controlled command execution into a WebKit-specific runner: add only the output observation seam needed by the auth launcher, retaining its process-group cancellation and reaping guarantees.

Represent completion as exit status plus a classified rendering failure, rather than parsing anyhow display text. Launch errors remain errors. Check exit status before decoding credential JSON so an aborted authenticator reports its failure rather than only "Failed to parse auth data". Discard partial credential output on every failed attempt.

Cancellation must close/kill and reap the owned process group, finish or cancel and join the pipe readers, and never start the retry. GUI launch futures currently use kill_on_drop; any new reader tasks need explicit ownership and cleanup when DesktopLauncher's select cancels them. Pipe draining after direct-child exit is bounded: inherited pipes held open by helpers must not block completion indefinitely; stop owned helpers and finish readers. User cancellation wins a race with classification at the retry decision.

## Desktop GUI recovery

GuiLauncher owns one GUI attempt: version resolution, process spawning, credential writing and observation. It returns the attempt outcome; DesktopLauncher owns the retry decision, rendering mode and lifetime budget. Resolve the executable once for the initial attempt and use that same selected executable on recovery; do not download another GUI after a rendering crash.

Do not resend an old service credential. Extend the private desktop control pipe with a named rendering-retry message, distinct from the existing GUI-exited message. After the failed child and its readers have stopped, DesktopLauncher sends that message. DesktopClient revokes the old session and issues a fresh credential, without requiring gui_restart_requested. Rendering renewal consumes and clears any pending gui_restart_requested intent from the old session, rather than leaving it to authorize a later unintended restart. It limits this explicit rendering renewal to once per desktop channel. Invalid/repeated recovery messages fail closed. Keep the existing ordinary restart message behavior and EOF shutdown behavior.

The parent then launches the same GUI with the fresh credential and child-only DMABUF override. Continue monitoring service exit and credential-channel errors throughout renewal and retry. Preserve initial minimized intent for startup recovery; legitimate later GUI restarts retain current non-minimized semantics. If renewal or retry fails, close the control input and let the service shut down normally. Report the original rendering failure plus the recovery failure, including exit statuses, without credential bytes.

Rendering recovery is not VPN recovery. No portal login, tunnel reconnection or restoration of an interrupted authentication is initiated by restarting the GUI. Revoking the old session follows existing service ownership/disconnect rules; do not promise uninterrupted VPN traffic after a GUI crash.

## Authentication recovery

SamlAuthLauncher owns one attempt and reports a typed recognized rendering failure after draining output and reaping the process group. Automatic auth retry is allowed only for the bundled embedded authenticator; a configured custom executable must not be restarted or overridden. External-browser and callback paths are unchanged.

The CLI connection workflow and gpgui PortalConnector own the auth recovery. On the typed failure, spend the connection's retry budget, select DMABUF-disabled auth mode, discard partial output, and obtain a fresh prelogin for the same portal/gateway with the existing profile, client certificate options and cancellation token. Re-evaluate the server's authentication mode from the fresh response. If it now chooses standard or external-browser auth, follow that path without imposing an embedded WebKit retry. Return the replacement Prelogin together with its Credential from the owning prelogin/auth operation, so callers use refreshed region and authentication metadata instead of keeping the original Prelogin. Preserve existing gateway standard-auth password fallback when fresh prelogin changes to Standard. Do not replay an already navigated SAML request, and do not repeat portal config retrieval or gateway login after they have succeeded.

Pass the selected child rendering mode through SamlAuthLauncher, applying it after into_non_root desktop restoration. Subsequent embedded auth windows in that connection use the selected mode but cannot spend another recovery budget. Preserve the one-shot --clean behavior: do not clear browser state again if the first attempt already consumed it. If the fresh prelogin fails or auth fails again, return that error with rendering-recovery context. Explicit cancellation remains AuthenticationCancelled rather than a crash error. Wrap every failure after spending the recovery budget in a typed terminal recovery error (except cancellation, which keeps its existing type). Every fallback owner must preserve and stop on this type: portal-to-gateway fallback, cached-credential fallback, automatic gateway candidate iteration, internal gateway iteration, and the GUI stops_gateway_attempts decision. Do not inspect anyhow message text. Exhausted recovery must never cause another authentication window through those paths.

The existing gpauth native auth window is opaque; confirm all embedded windows and backing surfaces remain opaque under this setting. GP Connect's GUI also launches gpauth through SamlAuthLauncher, so both caller paths are covered.

## Opaque window appearance

In gpgui, derive immutable process rendering mode at startup on Linux/BSD: WEBKIT_DISABLE_DMABUF_RENDERER exactly `1` means opaque windows. This applies equally to manual overrides and automatic recovery. No extra production environment variable or persisted setting is needed; remove the VM-only GP_TEST_OPAQUE mechanism when implementing production behavior.

Extend the existing WindowAppearance value with `opaqueWindow`. One resolved value supplies main, settings and update native builders, initial URL data, generated bindings and runtime appearance notifications. Set native transparency false before creating any webview, disable outer corner rounding regardless of the saved preference, and remove outer modal clipping. Do not mutate the saved rounded-corner preference. Disable the corner preference control while opaque mode is active so it does not promise an unavailable effect.

Use the existing theme background token for HTML, body and root, including first paint, light/dark mode and runtime theme switches. The inline head bootstrap in each of the three HTML entrypoints reads opaqueWindow and applies the initial appearance before AppShell CSS or React loads; renderToRoot initialization alone is too late to guarantee the first paint. Native backing color must agree with the initial theme where supported, without a transparent flash. The existing Rust theme application path owns native background updates for all existing windows, including system-theme change events, while frontend theme handling updates the shared CSS tokens. New windows resolve the current theme before construction; AppHandle.set_theme alone is not sufficient for explicit native backing colors. Internal buttons/cards retain their normal rounding. Keep custom title bars and their dimensions. An update window created later receives the same immutable rendering mode. macOS panel configuration is unchanged.

## Logging and diagnostics

Log one warning before recovery: the app exited with a Wayland rendering error and is being restarted with DMABUF disabled (opaque windows for GUI). Preserve the failed-attempt stderr in the normal log destination. On second failure, report both attempts' exit statuses and a useful instruction to attach diagnostics; never dump auth stdout, command arguments or environment maps. Add the effective DMABUF setting to diagnostics only if not already collected. Do not send new telemetry or persist an automatic override.

## Verification before implementation is accepted

- Process fixtures emit the real diagnostic across chunk boundaries and with large stdout/stderr. Verify no deadlock, one retry, unchanged stdout JSON, original stderr delivery, bounds and actual final errors.
- Negative fixtures: preflight warning quoting the diagnostic, normal exit, nonzero auth rejection, unrelated abort, explicit override values, custom auth executable, external browser, X11, cancellation and exhausted retry budget.
- Desktop pipe integration: fresh credential/session ID on rendering renewal; old credential revoked; repeated renewal rejected; ordinary restart still works; a pending ordinary restart followed by rendering renewal does not authorize another unrequested exit/restart; service EOF/error during renewal does not hang or leave a child. Test cancellation at spawn, credential write, reader wait and retry transition.
- Auth workflow fixtures for CLI and GUI: two distinct prelogin requests, no SAML request replay, recovery shared across portal/gateway, no new clean-state reset, no outer fallback after terminal rendering failure, valid second credential used once, and cancellation during fresh prelogin.
- Opaque appearance tests and frontend build: theme-aware backgrounds, all window types, saved corner preference retained but forced off, no macOS effect.
- Run targeted gp Rust checks/tests and gpgui Rust check/frontend build using the actual sibling gp checkout.
- Repeat the KDE transparent/opaque comparison with production code: main/settings/update, hover, input focus, hide/show and theme changes. Use a synthetic child crash to exercise supervision on the VM; report separately from real rendering tests.
- Confirm the real crash workaround on the affected NVIDIA system when available. Linux KDE testing does not prove FreeBSD/OpenBSD behavior; perform supported cross-target checks and explicitly report remaining runtime gaps.

## Implementation order

1. Shared classification/observation and attempt outcomes with process fixtures.
2. Desktop credential renewal and bounded GUI recovery.
3. Theme-aware opaque appearance in gpgui and verification on KDE.
4. Fresh-prelogin auth recovery in both owning workflows.
5. End-to-end cancellation/negative tests, diagnostics/docs and independent implementation review.

No release or deployment is included in this change.

## Design review outcome

Three independent agents reviewed process/service lifecycle, authentication workflows, and GUI appearance against the current code. Their findings were incorporated and the revised sections reviewed again. All three reported no remaining blocking design findings.

Changes from review: retain early credential BrokenPipe long enough to classify a confirmed crash; clear old-session restart intent during credential renewal; propagate replacement prelogin metadata and preserve standard gateway password reuse; stop every fallback path on terminal recovery failure; define budgets per serialized connection attempt; initialize opacity before CSS loads; and keep native backing colors synchronized through existing theme ownership.

Design review preceded implementation. Three independent implementation reviews covered process/service lifecycle, authentication workflows and GUI appearance. Findings were fixed and re-reviewed with no remaining blockers.

Entrypoint follow-up: code inspection confirmed the release gpgui command delegates before creating a webview. The design now explicitly covers that route, its stdin recursion guard, minimized/environment forwarding and release-build verification; debug direct-launch supervision remains outside the production retry scope.

## Implementation verification

Both repos compile against the actual sibling checkout. Targeted checks passed: 41 gpapi process tests; six desktop-service tests (one subprocess helper ignored); six CLI auth-flow tests; four GUI auth recovery tests; 16 GUI utility tests; nine frontend appearance/settings tests and the frontend build.

On Fedora 44 KDE Wayland (Plasma/KWin 6.6.4, WebKitGTK 2.54.1, VMware vmwgfx), a VM-only fixture emitted the reported GDK protocol-error diagnostic and aborted the first supervised child. Both public launch paths recovered once, delivered a fresh credential/session ID and launched the real opaque GUI. Main/settings remained fully rendered during focus changes. Minimized intent survived retry, and activating the existing GUI did not start another child. Explicit DMABUF=0 prevented retry. Two recognized crashes stopped with both failures visible. Ctrl+C cleanup passed through both public paths after Linux/BSD gpgui bootstrap was changed to process replacement.

This validates supervision using a simulated crash, not the actual NVIDIA failure. The VM build used release entrypoints with optimization disabled. Update-window runtime appearance, live system-theme changes, affected NVIDIA hardware and FreeBSD/OpenBSD runtime remain unverified; window construction and appearance behavior are covered by inspection and targeted tests. Detailed evidence is in workspace gp_logs/kde-crash-recovery-2026-10-11/.

Manual KDE confirmation: after the simulated first-launch crash, the user tested the recovered real GUI and reported no flickering. This confirms the tested VM behavior; affected NVIDIA hardware and BSD runtime remain separate verification gaps.

Helper follow-up: two independent implementation reviews found no blockers. The helper shares the GUI's monitored child runner and desktop recovery budget. Local process tests passed (45), Linux VM process tests passed (48), and the sibling gpgui check passed. A simulated helper GDK error/SIGABRT renewed the credential and launched the real opaque helper with DMABUF disabled. Clicking and changing keyboard focus preserved its content. Cancellation reaped the test children; repeated crashes stopped after two attempts; ordinary failures and an explicit DMABUF override did not retry. The VM used a mock service credential channel and blocked downloads to avoid changing the installed application, so successful installation and the complete helper-to-GUI transition were not tested. Evidence is in workspace gp_logs/helper-crash-recovery-2026-10-12/.
