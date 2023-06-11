import { Box } from "@mui/material";
import { useAtomValue } from "jotai";
import "./App.css";
import { statusReadyAtom } from "./atoms/status";
import ConnectForm from "./components/ConnectForm";
import ConnectionStatus from "./components/ConnectionStatus";
import Feedback from "./components/Feedback";
import GatewaySwitcher from "./components/GatewaySwitcher";
import MainMenu from "./components/MainMenu";
import Notification from "./components/Notification";

function Loading() {
  return (
    <Box
      sx={{
        position: "absolute",
        inset: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
      }}
    >
      Loading...
    </Box>
  );
}

function MainContent() {
  return (
    <>
      <MainMenu />
      <ConnectionStatus />
      <ConnectForm />
      <GatewaySwitcher />
      <Feedback />
    </>
  );
}

export default function App() {
  const ready = useAtomValue(statusReadyAtom);

  return (
    <Box padding={2} paddingBottom={0}>
      {ready ? <MainContent /> : <Loading />}
      <Notification />
    </Box>
  );
}
