/* Exercise the production parser and probe dispatch, inspecting packets at the
 * encryption boundary. No VPN server, privileged networking, or real keys. */
#define construct_esp_packet inspect_esp_packet
#define openconnect_open_https fixture_open_https
#include "gpst.c"
#undef construct_esp_packet
#undef openconnect_open_https
#include "gpst_nlb.c"
#include <assert.h>

static const char *expected_destination;
static int packets_inspected;
static enum openconnect_tunnel_event observed_events[4];
static int observed_count;
static int ssl_response_valid;

int fixture_open_https(struct openconnect_info *vpninfo)
{
	return 0;
}

static int fixture_ssl_write(struct openconnect_info *vpninfo, char *buf, size_t len)
{
	return len;
}

static int fixture_ssl_read(struct openconnect_info *vpninfo, char *buf, size_t len)
{
	const char *response = ssl_response_valid ? "START_TUNNEL" : "invalid";
	assert(len >= strlen(response));
	memcpy(buf, response, strlen(response));
	return strlen(response);
}

static void observe_transport(void *data, enum openconnect_tunnel_event event)
{
	assert(observed_count < 4);
	observed_events[observed_count++] = event;
}

static void progress(void *data, int level, const char *format, ...)
{
	if (level > PRG_INFO)
		return;
	va_list args;
	va_start(args, format);
	vfprintf(stderr, format, args);
	va_end(args);
}

int inspect_esp_packet(struct openconnect_info *vpninfo, struct pkt *pkt, uint8_t next_header)
{
	struct in_addr destination;
	struct ip *ip = (void *)pkt->data;
	struct icmp *icmp = (void *)(pkt->data + sizeof(*ip));
	const unsigned char *payload;
	size_t payload_len;

	assert(inet_pton(AF_INET, expected_destination, &destination) == 1);
	assert(next_header == IPPROTO_IPIP);
	assert(ip->ip_v == 4 && ip->ip_p == IPPROTO_ICMP);
	assert(ip->ip_dst.s_addr == destination.s_addr);
	assert(ip->ip_src.s_addr == inet_addr("10.253.0.120"));
	assert(icmp->icmp_type == ICMP_ECHO);
	if (vpninfo->gp_nlb.enabled)
		payload = gpst_nlb_probe_payload(&payload_len);
	else {
		payload = (const unsigned char *)magic_ping_payload_gcm;
		payload_len = sizeof(magic_ping_payload_gcm);
	}
	assert(pkt->len == sizeof(*ip) + ICMP_MINLEN + payload_len);
	assert(!memcmp(pkt->data + sizeof(*ip) + ICMP_MINLEN, payload, payload_len));
	packets_inspected++;
	/* Send a placeholder datagram over a local socket pair. */
	return 1;
}

int main(int argc, char **argv)
{
	static const char key[] =
		"000000000000000000000000000000000000000000000000000000000000000000000000";
	static const char nlb_config[] =
		"<hs-key><bits>256</bits><val>"
		"0000000000000000000000000000000000000000000000000000000000000000"
		"</val></hs-key><enc-hs-key>fixture</enc-hs-key>"
		"<tunnel-opaque><val>fixture</val><valid-period>3600</valid-period></tunnel-opaque>";
	const char *gateway = "0.0.0.0", *default_gateway = "192.0.2.10";
	char xml[4096], gateway_xml[128];
	struct sockaddr_in peer = { .sin_family = AF_INET };
	struct openconnect_info *vpninfo;
	xmlDoc *doc;
	int sockets[2], nlb, invalid, length;

	assert(argc == 2);
	nlb = !strncmp(argv[1], "nlb-", 4);
	invalid = !strcmp(argv[1], "regular-invalid") || !strcmp(argv[1], "regular-missing");
	if (!strcmp(argv[1], "regular-address") || !strcmp(argv[1], "nlb-address"))
		gateway = "192.0.2.1";
	else if (!strcmp(argv[1], "regular-invalid"))
		gateway = "invalid";
	if (!strcmp(argv[1], "nlb-client-vip"))
		default_gateway = "0.0.0.0";
	if (!strcmp(argv[1], "regular-missing"))
		gateway_xml[0] = 0;
	else
		snprintf(gateway_xml, sizeof(gateway_xml), "<gw-address>%s</gw-address>", gateway);
	length = snprintf(xml, sizeof(xml),
		"<response>%s<ip-address>10.253.0.120</ip-address>"
		"<default-gateway>%s</default-gateway><ipsec>"
		"<udp-port>4501</udp-port><enc-algo>aes-256-gcm</enc-algo><hmac-algo>sha1</hmac-algo>"
		"<c2s-spi>0x12345678</c2s-spi><s2c-spi>0x87654321</s2c-spi>"
		"<ekey-c2s><bits>288</bits><val>%s</val></ekey-c2s>"
		"<ekey-s2c><bits>288</bits><val>%s</val></ekey-s2c>"
		"</ipsec>%s</response>", gateway_xml, default_gateway, key, key, nlb ? nlb_config : "");
	assert(length > 0 && length < sizeof(xml));
	doc = xmlReadMemory(xml, length, "config.xml", NULL, XML_PARSE_NONET);
	assert(doc);
	vpninfo = openconnect_vpninfo_new("ESP gateway test", NULL, NULL, NULL, progress, NULL);
	assert(vpninfo);
	assert(!openconnect_set_protocol(vpninfo, "gp"));
	vpninfo->peer_addr = malloc(sizeof(peer));
	assert(vpninfo->peer_addr);
	memcpy(vpninfo->peer_addr, &peer, sizeof(peer));
	assert(!gpst_parse_config_xml(vpninfo, xmlDocGetRootElement(doc), NULL));
	xmlFreeDoc(doc);
	assert(!!vpninfo->gp_nlb.enabled == nlb);
	if (invalid) {
		assert(!vpninfo->esp_magic_af);
		assert(!vpninfo->delay_tunnel_reason);
		openconnect_vpninfo_free(vpninfo);
		return 0;
	}
	assert(vpninfo->esp_magic_af == AF_INET);
	assert(!memcmp(vpninfo->esp_magic, &(struct in_addr){ .s_addr = inet_addr(gateway) }, 4));
	assert(vpninfo->delay_tunnel_reason);
	assert(vpninfo->dtls_state == DTLS_SECRET);
	assert(!socketpair(AF_UNIX, SOCK_DGRAM, 0, sockets));
	vpninfo->dtls_fd = sockets[0];
	if (nlb) {
		const char *vip = gateway;
		if (!strcmp(gateway, "0.0.0.0")) {
			vip = default_gateway;
			if (!strcmp(default_gateway, "0.0.0.0"))
				vip = "10.253.0.120";
		}
		assert(vpninfo->gp_nlb.tunnel_vip);
		assert(!strcmp(vpninfo->gp_nlb.tunnel_vip, vip));
		assert(!gpst_nlb_prepare(vpninfo));
		assert(vpninfo->gp_nlb.control_state == GP_NLB_CONTROL_PENDING);
		assert(!gpst_nlb_control_ready(vpninfo));
		assert(!memcmp(vpninfo->esp_magic, &(struct in_addr){ .s_addr = inet_addr(vip) }, 4));
		/* Model completed tunnel control: keepalives must use the real inner
		 * gateway, not the zero probe address or the initial virtual IP. */
		vpninfo->gp_nlb.control_state = GP_NLB_CONTROL_READY;
		vpninfo->gp_nlb.inner_gw_ip = strdup("192.0.2.20");
		expected_destination = "192.0.2.20";
		assert(!gpst_esp_send_probes(vpninfo));
		assert(packets_inspected == 3);
		assert(vpninfo->gp_nlb.keepalive_sent);
		free(vpninfo->gp_nlb.inner_gw_ip);
		vpninfo->gp_nlb.inner_gw_ip = NULL;
		expected_destination = vip;
		assert(!gpst_nlb_send_esp_keepalive(vpninfo));
		assert(packets_inspected == 4);
	} else {
		assert(!vpninfo->gp_nlb.tunnel_vip || strcmp(vpninfo->gp_nlb.tunnel_vip, "0.0.0.0"));
		expected_destination = gateway;
		assert(!gpst_esp_send_probes(vpninfo));
		assert(packets_inspected == 1);
		/* Configuration/probes alone do not advertise ESP. Exercise the actual
		 * GP activation and ESP shutdown paths rather than synthesizing events. */
		int timeout = 1000;
		openconnect_set_tunnel_handler(vpninfo, observe_transport);
		assert(observed_count == 0);
		vpninfo->dtls_state = DTLS_CONNECTED;
		memset(&vpninfo->ssl_times, 0, sizeof(vpninfo->ssl_times));
		vpninfo->trojan_interval = 0;
		assert(!gpst_mainloop(vpninfo, &timeout, 0));
		assert(vpninfo->dtls_state == DTLS_ESTABLISHED);
		assert(observed_count == 1 && observed_events[0] == OC_TUNNEL_IPSEC_ESP);
		esp_close(vpninfo);
		assert(observed_count == 2 && observed_events[1] == OC_TUNNEL_UNAVAILABLE);
		/* Repeated shutdown of an inactive transport cannot claim another loss. */
		esp_close(vpninfo);
		assert(observed_count == 2);
		/* Replace only HTTPS I/O: run the actual SSL response acceptance path. */
		vpninfo->dtls_state = DTLS_DISABLED;
		vpninfo->ssl_read = fixture_ssl_read;
		vpninfo->ssl_write = fixture_ssl_write;
		openconnect_set_cookie(vpninfo, "user=test&authcookie=fixture");
		free(vpninfo->urlpath);
		vpninfo->urlpath = strdup("ssl-tunnel-connect.sslvpn");
		assert(gpst_connect(vpninfo) == -EINVAL);
		assert(observed_count == 2);
		ssl_response_valid = 1;
		assert(!gpst_connect(vpninfo));
		assert(observed_count == 3 && observed_events[2] == OC_TUNNEL_SSL_TLS);
	}
	close(sockets[1]);
	openconnect_vpninfo_free(vpninfo);
	return 0;
}
