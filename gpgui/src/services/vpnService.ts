import { invoke } from "@tauri-apps/api";
import { listen } from '@tauri-apps/api/event';

type Status = 'disconnected' | 'connecting' | 'connected' | 'disconnecting'
type StatusCallback = (status: Status) => void
type StatusEvent = {
  payload: {
    status: Status
  }
}

class VpnService {
  private _status: Status = 'disconnected';
  private statusCallbacks: StatusCallback[] = [];

  constructor() {
    this.init();
  }

  private async init() {
    const unlisten = await listen('vpn-status-received', (event: StatusEvent) => {
      console.log('vpn-status-received', event.payload)
      this.setStatus(event.payload.status);
    })

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
    return this.invokeCommand<Status>("vpn_status");
  }

  async connect(server: string, cookie: string) {
    return this.invokeCommand("vpn_connect", { server, cookie });
  }

  async disconnect() {
    return this.invokeCommand("vpn_disconnect");
  }

  onStatusChanged(callback: StatusCallback) {
    this.statusCallbacks.push(callback);
    callback(this._status);
    return () => this.removeStatusCallback(callback);
  }

  private fireStatusCallbacks() {
    this.statusCallbacks.forEach(cb => cb(this._status));
  }

  private removeStatusCallback(callback: StatusCallback) {
    this.statusCallbacks = this.statusCallbacks.filter(cb => cb !== callback);
  }

  private async invokeCommand<T>(command: string, args?: any) {
    try {
      return await invoke<T>(command, args);
    } catch (err: any) {
      throw new Error(err.message);
    }
  }
}

export default new VpnService();
