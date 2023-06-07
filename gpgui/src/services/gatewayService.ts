import { Body, ResponseType, fetch } from "@tauri-apps/api/http";
import { parseXml } from "../utils/parseXml";

type LoginParams = {
  user: string;
  passwd?: string | null;
  userAuthCookie?: string | null;
  prelogonUserAuthCookie?: string | null;
};

class GatewayService {
  async login(gateway: string, params: LoginParams) {
    const { user, passwd, userAuthCookie, prelogonUserAuthCookie } = params;
    if (!gateway) {
      throw new Error("Gateway address is required");
    }

    const loginUrl = `https://${gateway}/ssl-vpn/login.esp`;
    const body = Body.form({
      prot: "https:",
      inputStr: "",
      jnlpReady: "jnlpReady",
      computer: "Linux", // TODO
      ok: "Login",
      direct: "yes",
      "ipv6-support": "yes",
      clientVer: "4100",
      clientos: "Linux",
      "os-version": "Linux",
      server: gateway,
      user,
      passwd: passwd || "",
      "prelogin-cookie": "",
      "portal-userauthcookie": userAuthCookie || "",
      "portal-prelogonuserauthcookie": prelogonUserAuthCookie || "",
    });

    const response = await fetch<string>(loginUrl, {
      method: "POST",
      headers: {
        "User-Agent": "PAN GlobalProtect",
      },
      responseType: ResponseType.Text,
      body,
    });

    if (!response.ok) {
      throw new Error("Login failed");
    }

    return this.parseLoginResponse(response.data);
  }

  private parseLoginResponse(response: string) {
    const result = parseXml(response);
    const query = new URLSearchParams();

    query.append("authcookie", result.text("argument:nth-child(2)"));
    query.append("portal", result.text("argument:nth-child(4)"));
    query.append("user", result.text("argument:nth-child(5)"));
    query.append("domain", result.text("argument:nth-child(8)"));
    query.append("preferred-ip", result.text("argument:nth-child(16)"));
    query.append("computer", "Linux");

    return query.toString();
  }
}

export default new GatewayService();
