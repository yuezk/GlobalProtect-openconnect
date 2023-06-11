import { Event, listen } from "@tauri-apps/api/event";
import invokeCommand from "../utils/invokeCommand";

type VpnStatus = "disconnected" | "connecting" | "connected" | "disconnecting";
type VpnStatusCallback = (status: VpnStatus) => void;
type VpnStatusPayload = {
  status: VpnStatus;
};

type ServiceStatusCallback = (status: boolean) => void;

class VpnService {
  private _isOnline?: boolean;
  private _status?: VpnStatus;
  private statusCallbacks: VpnStatusCallback[] = [];
  private serviceStatusCallbacks: ServiceStatusCallback[] = [];

  constructor() {
    this.init();
  }

  private async init() {
    await listen("service-status-changed", (event: Event<boolean>) => {
      this.setIsOnline(event.payload);
    });

    await listen("vpn-status-received", (event: Event<VpnStatusPayload>) => {
      this.setStatus(event.payload.status);
    });
  }

  async isOnline() {
    try {
      const isOnline = await invokeCommand<boolean>("service_online");
      this.setIsOnline(isOnline);
      return isOnline;
    } catch (err) {
      return false;
    }
  }

  private setIsOnline(isOnline: boolean) {
    if (this._isOnline !== isOnline) {
      this._isOnline = isOnline;
      this.serviceStatusCallbacks.forEach((cb) => cb(isOnline));
    }
  }

  private setStatus(status: VpnStatus) {
    if (this._status !== status) {
      this._status = status;
      this.statusCallbacks.forEach((cb) => cb(status));
    }
  }

  async status(): Promise<VpnStatus> {
    try {
      const status = await invokeCommand<VpnStatus>("vpn_status");
      this._status = status;
      return status;
    } catch (err) {
      return "disconnected";
    }
  }

  async connect(server: string, cookie: string) {
    return invokeCommand("vpn_connect", { server, cookie });
  }

  async disconnect() {
    return invokeCommand("vpn_disconnect");
  }

  onVpnStatusChanged(callback: VpnStatusCallback) {
    this.statusCallbacks.push(callback);
    if (typeof this._status === "string") {
      callback(this._status);
    }
    return () => this.removeVpnStatusCallback(callback);
  }

  onServiceStatusChanged(callback: ServiceStatusCallback) {
    this.serviceStatusCallbacks.push(callback);
    if (typeof this._isOnline === "boolean") {
      callback(this._isOnline);
    }
    return () => this.removeServiceStatusCallback(callback);
  }

  private removeVpnStatusCallback(callback: VpnStatusCallback) {
    this.statusCallbacks = this.statusCallbacks.filter((cb) => cb !== callback);
  }

  private removeServiceStatusCallback(callback: ServiceStatusCallback) {
    this.serviceStatusCallbacks = this.serviceStatusCallbacks.filter(
      (cb) => cb !== callback
    );
  }
}

export default new VpnService();
