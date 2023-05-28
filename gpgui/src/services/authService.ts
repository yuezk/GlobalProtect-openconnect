import { Event, emit, listen } from "@tauri-apps/api/event";
import invokeCommand from "../utils/invokeCommand";

type AuthData = {
  username: string;
  prelogin_cookie: string | null;
  portal_userauthcookie: string | null;
};

class AuthService {
  private authSuccessCallback: ((data: AuthData) => void) | undefined;
  private authErrorCallback: (() => void) | undefined;
  private authCancelCallback: (() => void) | undefined;

  constructor() {
    this.init();
  }

  private async init() {
    await listen("auth-success", (event: Event<AuthData>) => {
      this.authSuccessCallback?.(event.payload);
    });
    await listen("auth-error", (event) => {
      this.authErrorCallback?.();
    });
    await listen("auth-cancel", (event) => {
      this.authCancelCallback?.();
    });
  }

  onAuthSuccess(callback: (data: AuthData) => void) {
    this.authSuccessCallback = callback;
  }

  onAuthError(callback: () => void) {
    this.authErrorCallback = callback;
  }

  onAuthCancel(callback: () => void) {
    this.authCancelCallback = callback;
  }

  // binding: "POST" | "REDIRECT"
  async samlLogin(binding: string, request: string) {
    return invokeCommand("saml_login", { binding, request });
  }

  emitAuthRequest(authRequest: string) {
    emit("auth-request", { samlRequest: authRequest });
  }
}

export default new AuthService();
