import { UserAttentionType, WebviewWindow } from "@tauri-apps/api/window";
import invokeCommand from "../utils/invokeCommand";
import { appStorage } from "./storageService";

export type TabValue = "simulation" | "openssl";
const SETTINGS_WINDOW_LABEL = "settings";

async function openSettings(options?: { tab?: TabValue }) {
  const tab = options?.tab || "simulation";
  const webview = WebviewWindow.getByLabel(SETTINGS_WINDOW_LABEL);

  if (webview) {
    await webview.requestUserAttention(UserAttentionType.Critical);
    return;
  }

  new WebviewWindow(SETTINGS_WINDOW_LABEL, {
    url: `pages/settings/index.html?tab=${tab}`,
    title: "GlobalProtect Settings",
    width: 650,
    height: 480,
    center: true,
    resizable: false,
    fileDropEnabled: false,
    focus: true,
  });
}

async function closeSettings() {
  const webview = WebviewWindow.getByLabel(SETTINGS_WINDOW_LABEL);
  if (webview) {
    await webview.close();
  }
}

async function getCurrentOsVersion() {
  return invokeCommand<string>("os_version");
}

export type ClientOS = "Linux" | "Windows" | "Mac";

export type SettingsData = {
  clientOS: ClientOS;
  osVersion: string;
  clientVersion: string;
  customOpenSSL: boolean;
};

type SimulationSettings = {
  userAgent: string;
  clientOS: ClientOS;
  osVersion: string;
  clientVersion: string;
};

export const SETTINGS_DATA = "SETTINGS_DATA";

const UA_PREFIX = "PAN GlobalProtect";
const DEFAULT_CLIENT_OS: ClientOS = "Linux";
const DEFAULT_OS_VERSION_MACOS = "Apple Mac OS X 13.4.0";
const DEFAULT_OS_VERSION_WINDOWS = "Microsoft Windows 11 Pro , 64-bit";
export const DEFAULT_CLIENT_VERSION = "6.0.1-19";

export const DEFAULT_SETTINGS_DATA: SettingsData = {
  clientOS: DEFAULT_CLIENT_OS,
  osVersion: "",
  clientVersion: "",
  customOpenSSL: false,
};

async function getSimulation(): Promise<SimulationSettings> {
  const { clientOS, osVersion, clientVersion } =
    (await appStorage.get<SettingsData>(SETTINGS_DATA)) ||
    DEFAULT_SETTINGS_DATA;
  const currentOsVersion = await getCurrentOsVersion();

  return {
    userAgent: buildUserAgent(
      clientOS,
      osVersion,
      currentOsVersion,
      clientVersion
    ),
    clientOS,
    osVersion: determineOsVersion(clientOS, osVersion, currentOsVersion),
    clientVersion: clientVersion || DEFAULT_CLIENT_VERSION,
  };
}

function buildUserAgent(
  clientOS: ClientOS,
  osVersion: string,
  currentOsVersion: string,
  clientVersion: string
) {
  osVersion = determineOsVersion(clientOS, osVersion, currentOsVersion);
  clientVersion = clientVersion || DEFAULT_CLIENT_VERSION;

  const suffix = ` (${clientOS === "Linux" ? "Linux " : ""}${osVersion})`;
  return `${UA_PREFIX}/${clientVersion}${suffix}`;
}

function determineOsVersion(
  clientOS: ClientOS,
  osVersion: string,
  currentOsVersion: string
) {
  if (osVersion.trim()) {
    return osVersion;
  }

  if (clientOS === "Linux") {
    return currentOsVersion;
  }

  if (clientOS === "Windows") {
    return DEFAULT_OS_VERSION_WINDOWS;
  }

  return DEFAULT_OS_VERSION_MACOS;
}

async function getOpenSSLConfig() {
  return invokeCommand("openssl_config");
}

async function updateOpenSSLConfig() {
  return invokeCommand("update_openssl_config");
}

export default {
  openSettings,
  closeSettings,
  getCurrentOsVersion,
  getSimulation,
  buildUserAgent,
  determineOsVersion,
  getOpenSSLConfig,
  updateOpenSSLConfig,
};
