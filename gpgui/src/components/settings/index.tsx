import { Devices, Https } from "@mui/icons-material";
import { TabContext, TabList } from "@mui/lab";
import { Box, Button, DialogActions, Tab } from "@mui/material";
import { useSetAtom } from "jotai";
import { useState } from "react";
import { saveSettingsAtom } from "../../atoms/settings";
import settingsService, { TabValue } from "../../services/settingsService";
import OpenSSL from "./OpenSSL";
import Simulation from "./Simulation";

const activeTab = new URLSearchParams(window.location.search).get(
  "tab"
) as TabValue;

export default function SettingsPanel() {
  const [value, setValue] = useState<TabValue>(activeTab);
  const saveSettings = useSetAtom(saveSettingsAtom);

  const handleChange = (event: React.SyntheticEvent, newValue: string) => {
    setValue(newValue as TabValue);
  };

  const closeWindow = async () => {
    await settingsService.closeSettings();
  };

  const save = async () => {
    await saveSettings();
    await closeWindow();
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
            />
            <Tab
              label="OpenSSL"
              value="openssl"
              icon={<Https />}
              iconPosition="start"
            />
          </TabList>
          <Box sx={{ flex: 1 }}>
            <Simulation />
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
