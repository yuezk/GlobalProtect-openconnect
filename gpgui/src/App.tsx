import { Box } from "@mui/material";
import ConnectForm from "./components/ConnectForm";
import ConnectionStatus from "./components/ConnectionStatus";
import Feedback from "./components/Feedback";
import Notification from "./components/Notification";

export default function App() {
  return (
    <Box padding={2} paddingTop={3}>
      <ConnectionStatus />
      <ConnectForm />
      <Feedback />
      <Notification />
    </Box>
  );
}
