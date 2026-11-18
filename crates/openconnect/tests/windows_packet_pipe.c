/* Exercise the DLL's packet mainloop without a VPN server or kernel adapter. */
#include "config.h"
#include "openconnect-internal.h"
#include <assert.h>

struct test_context {
	wchar_t name[128];
	SOCKET command;
	int mode, seeded;
	volatile LONG statistics;
};
static void progress(void *data, int level, const char *format, ...)
{
	(void)data; (void)level; (void)format;
}
static void statistics(void *data, const struct oc_stats *stats)
{
	struct test_context *context = data;
	(void)stats;
	InterlockedIncrement(&context->statistics);
}
static int pump(struct openconnect_info *vpninfo, int *timeout, int readable)
{
	struct test_context *context = vpninfo->cbdata;
	struct pkt *packet;
	int work = 0;
	(void)timeout; (void)readable;
	vpninfo->need_poll_cmd_fd = 1;
	while ((packet = dequeue_packet(&vpninfo->outgoing_queue))) {
		queue_packet(&vpninfo->incoming_queue, packet);
		work = 1;
	}
	if (context->mode == 2 && !context->seeded) {
		int index;
		context->seeded = 1;
		for (index = 0; index < 32; index++) {
			packet = calloc(1, sizeof(*packet) + 1500);
			assert(packet);
			packet->len = 1500;
			memset(packet->data, index, packet->len);
			queue_packet(&vpninfo->incoming_queue, packet);
		}
		work = 1;
	}
	return work;
}
static const struct vpn_proto protocol = { .name = "packet-test", .tcp_mainloop = pump };
static DWORD WINAPI client(void *data)
{
	struct test_context *context = data;
	HANDLE pipe = CreateFileW(context->name, GENERIC_READ | GENERIC_WRITE, 0, NULL,
				  OPEN_EXISTING, 0, NULL);
	DWORD mode = PIPE_READMODE_MESSAGE, count;
	char bytes[1024], response[1024], command;
	assert(pipe != INVALID_HANDLE_VALUE);
	assert(SetNamedPipeHandleState(pipe, &mode, NULL, NULL));
	if (context->mode == 0) {
		int index;
		for (index = 0; index < 2; index++) {
			DWORD length = 257 + index * 97;
			memset(bytes, 'a' + index, length);
			assert(WriteFile(pipe, bytes, length, &count, NULL) && count == length);
			assert(ReadFile(pipe, response, sizeof(response), &count, NULL) && count == length);
			assert(!memcmp(bytes, response, length));
		}
	} else if (context->mode == 3) {
		/* Free before mainloop must close the duplicate and drain its read. */
		assert(!ReadFile(pipe, response, sizeof(response), &count, NULL));
		assert(GetLastError() == ERROR_BROKEN_PIPE);
	} else {
		/* No reads: mode 2 fills the service-to-worker packet buffer. */
		Sleep(100);
		command = OC_CMD_STATS;
		assert(send(context->command, &command, 1, 0) == 1);
		Sleep(100);
		command = OC_CMD_CANCEL;
		assert(send(context->command, &command, 1, 0) == 1);
		Sleep(100);
	}
	CloseHandle(pipe);
	return 0;
}
static DWORD WINAPI watchdog(void *event)
{
	if (WaitForSingleObject(event, 15000) == WAIT_TIMEOUT) {
		fputs("FAIL: packet pipe test timed out\n", stderr);
		ExitProcess(99);
	}
	return 0;
}
static void run(int mode)
{
	struct test_context context = { .mode = mode };
	struct openconnect_info *vpninfo;
	HANDLE pipe, thread, event;
	OVERLAPPED connected = { 0 };
	DWORD count;
	ULONGLONG started;
	int result;
	swprintf(context.name, 128, L"\\\\.\\pipe\\GPConnect.NativePacketTest.%lu.%d", GetCurrentProcessId(), mode);
	pipe = CreateNamedPipeW(context.name, PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
		PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_REJECT_REMOTE_CLIENTS, 1, 4096, 4096, 0, NULL);
	assert(pipe != INVALID_HANDLE_VALUE);
	vpninfo = openconnect_vpninfo_new("packet-test", NULL, NULL, NULL, progress, &context);
	assert(vpninfo);
	context.command = openconnect_setup_cmd_pipe(vpninfo);
	assert(context.command != INVALID_SOCKET);
	vpninfo->proto = &protocol;
	vpninfo->dtls_state = DTLS_DISABLED;
	vpninfo->ip_info.mtu = 1500;
	openconnect_set_stats_handler(vpninfo, statistics);
	thread = CreateThread(NULL, 0, client, &context, 0, NULL);
	assert(thread);
	event = CreateEvent(NULL, TRUE, FALSE, NULL);
	assert(event);
	connected.hEvent = event;
	if (!ConnectNamedPipe(pipe, &connected)) {
		DWORD error = GetLastError();
		if (error != ERROR_PIPE_CONNECTED) {
			assert(error == ERROR_IO_PENDING);
			assert(GetOverlappedResult(pipe, &connected, &count, TRUE));
		}
	}
	CloseHandle(event);
	assert(!openconnect_setup_tun_pipe(vpninfo, (intptr_t)pipe));
	assert(vpninfo->tun_is_pipe && vpninfo->tun_fh != pipe);
	CloseHandle(pipe);
	started = GetTickCount64();
	if (mode == 3) {
		vpninfo->tun_pkt = calloc(1, sizeof(struct pkt) + 1500);
		assert(vpninfo->tun_pkt);
		assert(!ReadFile(vpninfo->tun_fh, vpninfo->tun_pkt->data, 1500, &count, &vpninfo->tun_rd_overlap));
		assert(GetLastError() == ERROR_IO_PENDING);
		vpninfo->tun_rd_pending = 1;
		openconnect_vpninfo_free(vpninfo);
	} else {
		result = openconnect_mainloop(vpninfo, 1, 1);
		assert(result < 0);
		assert(vpninfo->tun_fh == INVALID_HANDLE_VALUE && !vpninfo->tun_rd_pending);
		assert(!vpninfo->tun_rd_overlap.hEvent && !vpninfo->tun_wr_overlap.hEvent);
		if (mode) {
			assert(GetTickCount64() - started < 3000);
			assert(context.statistics == 1);
			if (mode == 2)
				assert(vpninfo->stats.rx_pkts > 0 && vpninfo->stats.rx_pkts < 32);
		} else {
			assert(vpninfo->stats.rx_pkts == 2 && vpninfo->stats.tx_pkts == 2);
		}
		openconnect_vpninfo_free(vpninfo);
	}
	assert(WaitForSingleObject(thread, 2000) == WAIT_OBJECT_0);
	CloseHandle(thread);
	printf("PASS: native packet pipe case %d\n", mode);
}
int main(void)
{
	HANDLE stop = CreateEvent(NULL, TRUE, FALSE, NULL);
	HANDLE guard = CreateThread(NULL, 0, watchdog, stop, 0, NULL);
	int mode;
	assert(stop && guard);
	assert(!openconnect_init_ssl());
	for (mode = 0; mode < 4; mode++)
		run(mode);
	SetEvent(stop);
	WaitForSingleObject(guard, 1000);
	CloseHandle(guard);
	CloseHandle(stop);
	return 0;
}
