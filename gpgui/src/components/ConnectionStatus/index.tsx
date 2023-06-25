import { Box } from "@mui/material";
import StatusIcon from "./StatusIcon";
import StatusText from "./StatusText";

export default function ConnectionStatus() {
  return (
    <Box data-tauri-drag-region>
      <StatusIcon />
      <StatusText />
    </Box>
  );
}
