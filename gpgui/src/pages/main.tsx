import { Box } from "@mui/material";
import { renderToRoot } from "../components/AppShell";
import ConnectForm from "../components/ConnectForm";
import ConnectionStatus from "../components/ConnectionStatus";
import Feedback from "../components/Feedback";
import GatewaySwitcher from "../components/GatewaySwitcher";
import MainMenu from "../components/MainMenu";
import Notification from "../components/Notification";

export default function App() {
  return (
    <Box data-tauri-drag-region padding={2} paddingBottom={0}>
      <MainMenu />
      <ConnectionStatus />
      <ConnectForm />
      <GatewaySwitcher />
      <Feedback />
      <Notification />
    </Box>
  );
}

renderToRoot(<App />);
