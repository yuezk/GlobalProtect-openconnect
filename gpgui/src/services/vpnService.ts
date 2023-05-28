import { Event, listen } from "@tauri-apps/api/event";
import invokeCommand from "../utils/invokeCommand";

type Status = "disconnected" | "connecting" | "connected" | "disconnecting";
type StatusCallback = (status: Status) => void;
type StatusPayload = {
  status: Status;
};

class VpnService {
  private _status: Status = "disconnected";
  private statusCallbacks: StatusCallback[] = [];

  constructor() {
    this.init();
  }

  private async init() {
    await listen("vpn-status-received", (event: Event<StatusPayload>) => {
      console.log("vpn-status-received", event.payload);
      this.setStatus(event.payload.status);
    });

    const status = await this.status();
    this.setStatus(status);
  }

  private setStatus(status: Status) {
    if (this._status != status) {
      this._status = status;
      this.fireStatusCallbacks();
    }
  }

  private async status(): Promise<Status> {
    return invokeCommand<Status>("vpn_status");
  }

  async connect(server: string, cookie: string) {
    return invokeCommand("vpn_connect", { server, cookie });
  }

  async disconnect() {
    return invokeCommand("vpn_disconnect");
  }

  onStatusChanged(callback: StatusCallback) {
    this.statusCallbacks.push(callback);
    callback(this._status);
    return () => this.removeStatusCallback(callback);
  }

  private fireStatusCallbacks() {
    this.statusCallbacks.forEach((cb) => cb(this._status));
  }

  private removeStatusCallback(callback: StatusCallback) {
    this.statusCallbacks = this.statusCallbacks.filter((cb) => cb !== callback);
  }
}

export default new VpnService();
