import { Body, ResponseType, fetch } from "@tauri-apps/api/http";
import { Maybe } from "../types";
import { parseXml } from "../utils/parseXml";
import { Gateway } from "./types";

type LoginParams = {
  gateway: Gateway;
  user: string;
  passwd: string;
  userAuthCookie: Maybe<string>;
};

class GatewayService {
  async login(params: LoginParams) {
    const { gateway, user, passwd, userAuthCookie } = params;
    if (!gateway.address) {
      throw new Error("Gateway address is required");
    }

    const loginUrl = `https://${gateway.address}/ssl-vpn/login.esp`;

    const response = await fetch<string>(loginUrl, {
      method: "POST",
      headers: {
        "User-Agent": "PAN GlobalProtect",
      },
      responseType: ResponseType.Text,
      body: Body.form({
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
        server: gateway.address,
        user,
        passwd,
        "portal-userauthcookie": userAuthCookie ?? "",
        "portal-prelogonuserauthcookie": "",
        "prelogin-cookie": "",
      }),
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
