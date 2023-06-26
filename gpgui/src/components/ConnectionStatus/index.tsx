import { Box } from "@mui/material";
import StatusIcon from "./StatusIcon";
import StatusText from "./StatusText";

export default function ConnectionStatus() {
  return (
    <Box position="relative">
      <StatusIcon />
      <StatusText />
      <Box data-tauri-drag-region position="absolute" sx={{ inset: 0 }} />
    </Box>
  );
}
