import { Body, ResponseType, fetch } from "@tauri-apps/api/http";
import { Maybe, MaybeProperties } from "../types";
import { parseXml } from "../utils/parseXml";
import authService from "./authService";
import { Gateway } from "./types";

type SamlPreloginResponse = {
  samlAuthMethod: string;
  samlAuthRequest: string;
};

type PasswordPreloginResponse = {
  labelUsername: string;
  labelPassword: string;
  authMessage: Maybe<string>;
};

type Region = {
  region: string;
};

type PreloginResponse = MaybeProperties<
  SamlPreloginResponse & PasswordPreloginResponse & Region
>;

type ConfigResponse = {
  userAuthCookie: Maybe<string>;
  prelogonUserAuthCookie: Maybe<string>;
  preferredGateway: Gateway;
  gateways: Gateway[];
};

class PortalService {
  async prelogin(portal: string) {
    const preloginUrl = `https://${portal}/global-protect/prelogin.esp`;

    const response = await fetch<string>(preloginUrl, {
      method: "GET",
      headers: {
        "User-Agent": "PAN GlobalProtect",
      },
      responseType: ResponseType.Text,
      query: {
        tmp: "tmp",
        "kerberos-support": "yes",
        "ipv6-support": "yes",
        clientVer: "4100",
        clientos: "Linux",
      },
    });

    if (!response.ok) {
      throw new Error(`Failed to connect to portal: ${response.status}`);
    }
    return this.parsePreloginResponse(response.data);
  }

  private parsePreloginResponse(response: string): PreloginResponse {
    const doc = parseXml(response);

    return {
      samlAuthMethod: doc.text("saml-auth-method").toUpperCase(),
      samlAuthRequest: atob(doc.text("saml-request")),
      labelUsername: doc.text("username-label"),
      labelPassword: doc.text("password-label"),
      authMessage: doc.text("authentication-message"),
      region: doc.text("region"),
    };
  }

  isSamlAuth(response: PreloginResponse): response is SamlPreloginResponse {
    if (response.samlAuthMethod && response.samlAuthRequest) {
      return true;
    }
    return false;
  }

  isPasswordAuth(
    response: PreloginResponse
  ): response is PasswordPreloginResponse {
    if (response.labelUsername && response.labelPassword) {
      return true;
    }
    return false;
  }

  async fetchConfig({
    portal,
    username,
    password,
  }: {
    portal: string;
    username: string;
    password: string;
  }) {
    const configUrl = `https://${portal}/global-protect/getconfig.esp`;
    const response = await fetch<string>(configUrl, {
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
        clientos: "Linux",
        ok: "Login",
        direct: "yes",
        clientVer: "4100",
        "os-version": "Linux",
        "ipv6-support": "yes",
        server: portal,
        user: username,
        passwd: password,
        "portal-userauthcookie": "",
        "portal-prelogonuserauthcookie": "",
        "prelogin-cookie": "",
      }),
    });

    if (!response.ok) {
      console.error(response);
      throw new Error(`Failed to fetch portal config: ${response.status}`);
    }

    return this.parsePortalConfigResponse(response.data);
  }

  private parsePortalConfigResponse(response: string): ConfigResponse {
    const result = parseXml(response);
    const gateways = result.all("gateways list > entry").map((entry) => {
      const address = entry.attr("name");
      const name = entry.text("description");
      const priority = entry.text(":scope > priority");

      return {
        name,
        address,
        priority: priority ? parseInt(priority, 10) : undefined,
        priorityRules: entry.all("priority-rule > entry").map((entry) => {
          const name = entry.attr("name");
          const priority = entry.text("priority");
          return {
            name,
            priority: priority ? parseInt(priority, 10) : undefined,
          };
        }),
      };
    });

    return {
      userAuthCookie: result.text("portal-userauthcookie"),
      prelogonUserAuthCookie: result.text("portal-prelogonuserauthcookie"),
      preferredGateway: gateways[0],
      gateways,
    };
  }
}

export default new PortalService();
