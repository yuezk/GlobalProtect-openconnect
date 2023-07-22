import { Devices, Https, VpnLock } from "@mui/icons-material";
import { TabContext, TabList } from "@mui/lab";
import { Box, Button, DialogActions, Tab } from "@mui/material";
import { useSetAtom } from "jotai";
import { useSnackbar } from "notistack";
import { useState } from "react";
import { saveSettingsAtom } from "../../atoms/settings";
import settingsService, { TabValue } from "../../services/settingsService";
import OpenConnect from "./OpenConnect";
import OpenSSL from "./OpenSSL";
import Simulation from "./Simulation";

const activeTab = new URLSearchParams(window.location.search).get(
  "tab"
) as TabValue;

export default function SettingsPanel() {
  const [value, setValue] = useState<TabValue>(activeTab);
  const saveSettings = useSetAtom(saveSettingsAtom);
  const { enqueueSnackbar } = useSnackbar();

  const handleChange = (event: React.SyntheticEvent, newValue: string) => {
    setValue(newValue as TabValue);
  };

  const closeWindow = async () => {
    await settingsService.closeSettings();
  };

  const save = async () => {
    try {
      await saveSettings();
      enqueueSnackbar("Settings saved", { variant: "success" });
      await closeWindow();
    } catch (err) {
      console.warn("Failed to save settings", err);
      enqueueSnackbar("Failed to save settings", { variant: "error" });
    }
  };

  return (
    <Box sx={{ height: "100%", display: "flex", flexDirection: "column" }}>
      <Box sx={{ flex: 1, height: 0, display: "flex" }}>
        <TabContext value={value}>
          <TabList
            onChange={handleChange}
            orientation="vertical"
            sx={{ borderRight: 1, borderColor: "divider", flexShrink: 0 }}
          >
            <Tab
              label="Simulation"
              value="simulation"
              icon={<Devices />}
              iconPosition="start"
              sx={{ justifyContent: "start" }}
            />
            <Tab
              label="OpenConnect"
              value="openconnect"
              icon={<VpnLock />}
              iconPosition="start"
              sx={{ justifyContent: "start" }}
            />
            <Tab
              label="OpenSSL"
              value="openssl"
              icon={<Https />}
              iconPosition="start"
              sx={{ justifyContent: "start" }}
            />
          </TabList>
          <Box
            sx={{
              flex: 1,
              display: "flex",
              flexDirection: "column",
              "& .MuiTabPanel-root[hidden]": { display: "none" },
            }}
          >
            <Simulation />
            <OpenConnect />
            <OpenSSL />
          </Box>
        </TabContext>
      </Box>
      <Box sx={{ flexShrink: 0, borderTop: 1, borderColor: "divider" }}>
        <DialogActions>
          <Button onClick={closeWindow}>Cancel</Button>
          <Button onClick={save}>Save</Button>
        </DialogActions>
      </Box>
    </Box>
  );
}
