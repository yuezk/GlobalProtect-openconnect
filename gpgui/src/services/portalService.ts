import { Body, ResponseType, fetch } from "@tauri-apps/api/http";
import { parseXml } from "../utils/parseXml";
import { Gateway } from "./types";

export type SamlPrelogin = {
  isSamlAuth: true;
  samlAuthMethod: string;
  samlRequest: string;
  region: string;
};

export type PasswordPrelogin = {
  isSamlAuth: false;
  authMessage: string;
  labelUsername: string;
  labelPassword: string;
  region: string;
};

export type Prelogin = SamlPrelogin | PasswordPrelogin;

export type PortalConfig = {
  userAuthCookie: string;
  prelogonUserAuthCookie: string;
  gateways: Gateway[];
};

export type PortalCredential = {
  user: string;
  passwd?: string; // for password auth
  "prelogin-cookie"?: string; // for saml auth
  "portal-userauthcookie"?: string; // cached cookie from previous portal config
  "portal-prelogonuserauthcookie"?: string; // cached cookie from previous portal config
};

class PortalService {
  async prelogin(portal: string): Promise<Prelogin> {
    const preloginUrl = `https://${portal}/global-protect/prelogin.esp`;
    let response;
    try {
      response = await fetch<string>(preloginUrl, {
        method: "POST",
        headers: {
          "User-Agent": "PAN GlobalProtect",
        },
        responseType: ResponseType.Text,
        query: {
          "kerberos-support": "yes",
        },
        body: Body.form({
          tmp: "tmp",
          clientVer: "4100",
          clientos: "Linux",
          "os-version": "Linux",
          "ipv6-support": "yes",
          "default-browser": "0",
          "cas-support": "yes",
          // "host-id": "TODO, mac address?",
        }),
      });
    } catch (err) {
      console.error("Failed to prelogin: Network error", err);
      throw new Error("Failed to prelogin: Network error");
    }

    if (!response.ok) {
      throw new Error(`Failed to prelogin: ${response.status}`);
    }
    return this.parsePrelogin(response.data);
  }

  private parsePrelogin(response: string): Prelogin {
    const doc = parseXml(response);
    const status = doc.text("status").toUpperCase();

    if (status !== "SUCCESS") {
      const message = doc.text("msg") || "Unknown error";
      throw new Error(message);
    }

    const samlAuthMethod = doc.text("saml-auth-method").toUpperCase();
    const samlRequest = doc.text("saml-request");
    const labelUsername = doc.text("username-label");
    const labelPassword = doc.text("password-label");
    const authMessage = doc.text("authentication-message");
    const region = doc.text("region");

    if (samlAuthMethod && samlRequest) {
      return {
        isSamlAuth: true,
        samlAuthMethod,
        samlRequest: atob(samlRequest),
        region,
      };
    }

    if (labelUsername && labelPassword) {
      return {
        isSamlAuth: false,
        authMessage,
        labelUsername,
        labelPassword,
        region,
      };
    }

    throw new Error("Unknown prelogin response");
  }

  async fetchConfig(portal: string, params: PortalCredential) {
    const {
      user,
      passwd,
      "prelogin-cookie": preloginCookie,
      "portal-userauthcookie": portalUserAuthCookie,
      "portal-prelogonuserauthcookie": portalPrelogonUserAuthCookie,
    } = params;

    const configUrl = `https://${portal}/global-protect/getconfig.esp`;
    const body = Body.form({
      prot: "https:",
      inputStr: "",
      jnlpReady: "jnlpReady",
      computer: "Linux", // TODO
      clientos: "Linux",
      ok: "Login",
      direct: "yes",
      clientVer: "4100",
      "os-version": "Linux",
      clientgpversion: "6.0.1-19",
      "ipv6-support": "yes",
      server: portal,
      host: portal,
      user,
      passwd: passwd || "",
      "prelogin-cookie": preloginCookie || "",
      "portal-userauthcookie": portalUserAuthCookie || "",
      "portal-prelogonuserauthcookie": portalPrelogonUserAuthCookie || "",
    });

    const response = await fetch<string>(configUrl, {
      method: "POST",
      headers: {
        "User-Agent": "PAN GlobalProtect",
      },
      responseType: ResponseType.Text,
      body,
    });

    if (!response.ok) {
      console.error(response);
      throw new Error(`Failed to fetch portal config: ${response.status}`);
    }

    return this.parsePortalConfigResponse(response.data);
  }

  private parsePortalConfigResponse(response: string): PortalConfig {
    // console.log(response);

    const result = parseXml(response);
    const gateways = result.all("gateways list > entry").map((entry) => {
      const address = entry.attr("name");
      const name = entry.text("description");
      const priority = entry.text(":scope > priority");

      return {
        name,
        address,
        priority: priority ? parseInt(priority, 10) : Infinity,
        priorityRules: entry.all("priority-rule > entry").map((entry) => {
          const name = entry.attr("name");
          const priority = entry.text("priority");
          return {
            name,
            priority: priority ? parseInt(priority, 10) : Infinity,
          };
        }),
      };
    });

    return {
      userAuthCookie: result.text("portal-userauthcookie"),
      prelogonUserAuthCookie: result.text("portal-prelogonuserauthcookie"),
      gateways,
    };
  }

  preferredGateway(
    gateways: Gateway[],
    { region, previousGateway }: { region: string; previousGateway?: string }
  ) {
    for (const gateway of gateways) {
      if (gateway.name === previousGateway) {
        return gateway;
      }
    }

    let defaultGateway = gateways[0];
    for (const gateway of gateways) {
      if (gateway.priority < defaultGateway.priority) {
        defaultGateway = gateway;
      }
    }

    if (!region) {
      return defaultGateway;
    }

    let preferredGateway = defaultGateway;
    let currentPriority = Infinity;
    for (const gateway of gateways) {
      const priorityRule = gateway.priorityRules.find(
        ({ name }) => name === region
      );

      if (priorityRule && priorityRule.priority < currentPriority) {
        preferredGateway = gateway;
        currentPriority = priorityRule.priority;
      }
    }
    return preferredGateway;
  }
}

export default new PortalService();
