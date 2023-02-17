import GppBadIcon from "@mui/icons-material/GppBad";
import VerifiedIcon from "@mui/icons-material/VerifiedUser";
import {
  Box,
  BoxProps,
  CircularProgress,
  Typography,
  useTheme,
} from "@mui/material";
import { BeatLoader } from "react-spinners";

export type Status =
  | "processing"
  | "disconnected"
  | "connecting"
  | "connected"
  | "disconnecting";

export const statusTextMap: Record<Status, string> = {
  processing: "Processing...",
  connected: "Connected",
  disconnected: "Not Connected",
  connecting: "Connecting...",
  disconnecting: "Disconnecting...",
};

export default function ConnectionStatus(
  props: BoxProps<"div", { status?: Status }>
) {
  const theme = useTheme();
  const { status = "disconnected" } = props;
  const { palette } = theme;
  const colorsMap: Record<Status, string> = {
    processing: palette.info.main,
    connected: palette.success.main,
    disconnected: palette.action.disabled,
    connecting: palette.info.main,
    disconnecting: palette.info.main,
  };

  const pending = ["processing", "connecting", "disconnecting"].includes(status);
  const connected = status === "connected";
  const disconnected = status === "disconnected";

  return (
    <Box {...props}>
      <Box
        sx={{
          textAlign: "center",
          position: "relative",
          width: 150,
          height: 150,
          mx: "auto",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        <CircularProgress
          size={150}
          thickness={1}
          value={pending ? undefined : 100}
          variant={pending ? "indeterminate" : "determinate"}
          sx={{
            position: "absolute",
            top: 0,
            left: 0,
            color: colorsMap[status],
            "& circle": {
              fill: colorsMap[status],
              fillOpacity: pending ? 0.1 : 0.25,
              transition: "all 0.3s ease",
            },
          }}
        />
        {pending && <BeatLoader color={colorsMap[status]} />}

        {connected && (
          <VerifiedIcon
            sx={{
              position: "relative",
              fontSize: 80,
              color: colorsMap[status],
            }}
          />
        )}

        {disconnected && (
          <GppBadIcon
            color="disabled"
            sx={{
              fontSize: 80,
              color: colorsMap[status],
            }}
          />
        )}
      </Box>

      <Typography textAlign="center" mt={1.5} variant="subtitle1" paragraph>
        {statusTextMap[status]}
      </Typography>
    </Box>
  );
}
