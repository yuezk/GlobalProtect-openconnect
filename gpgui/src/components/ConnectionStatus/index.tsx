import { Box } from "@mui/material";
import StatusIcon from "./StatusIcon";
import StatusText from "./StatusText";

export default function ConnectionStatus() {
  return (
    <Box>
      <StatusIcon />
      <StatusText />
    </Box>
  );
}
