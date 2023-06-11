import { emit, listen } from "@tauri-apps/api/event";
import invokeCommand from "../utils/invokeCommand";

export type AuthData = {
  username: string;
  prelogin_cookie?: string;
  portal_userauthcookie?: string;
};

class AuthService {
  private authErrorCallback: (() => void) | undefined;

  constructor() {
    this.init();
  }

  private async init() {
    await listen("auth-error", (evt) => {
      console.error("auth-error", evt);
      this.authErrorCallback?.();
    });
  }

  onAuthError(callback: () => void) {
    this.authErrorCallback = callback;
    return () => {
      this.authErrorCallback = undefined;
    };
  }

  // binding: "POST" | "REDIRECT"
  async samlLogin(binding: string, request: string, clearCookies: boolean) {
    return invokeCommand<AuthData>("saml_login", {
      binding,
      request,
      clearCookies,
    });
  }

  async emitAuthRequest({
    samlBinding,
    samlRequest,
  }: {
    samlBinding: string;
    samlRequest: string;
  }) {
    await emit("auth-request", { samlBinding, samlRequest });
  }
}

export default new AuthService();
