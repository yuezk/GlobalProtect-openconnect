import { GppBad, VerifiedUser as VerifiedIcon } from "@mui/icons-material";
import { Box, CircularProgress, styled, useTheme } from "@mui/material";
import { useAtomValue } from "jotai";
import { BeatLoader } from "react-spinners";
import { isProcessingAtom, statusAtom } from "../../atoms/status";

function useStatusColor() {
  const status = useAtomValue(statusAtom);
  const theme = useTheme();

  if (status === "disconnected") {
    return theme.palette.action.disabled;
  }

  if (status === "connected") {
    return theme.palette.success.main;
  }

  if (status === "error") {
    return theme.palette.error.main;
  }

  return theme.palette.info.main;
}

function BackgroundIcon() {
  const color = useStatusColor();
  const isProcessing = useAtomValue(isProcessingAtom);

  return (
    <CircularProgress
      size={150}
      thickness={1}
      value={isProcessing ? undefined : 100}
      variant={isProcessing ? "indeterminate" : "determinate"}
      sx={{
        position: "absolute",
        top: 0,
        left: 0,
        color,
        "& circle": {
          fill: color,
          fillOpacity: isProcessing ? 0.1 : 0.25,
          transition: "all 0.3s ease",
        },
      }}
    />
  );
}

const DisconnectedIcon = styled(GppBad)(({ theme }) => ({
  position: "relative",
  fontSize: 90,
  color: theme.palette.action.disabled,
}));

function ProcessingIcon() {
  const theme = useTheme();
  return <BeatLoader color={theme.palette.info.main} />;
}

const ConnectedIcon = styled(VerifiedIcon)(({ theme }) => ({
  position: "relative",
  fontSize: 80,
  color: theme.palette.success.main,
}));

const IconContainer = styled(Box)(({ theme }) =>
  theme.unstable_sx({
    position: "relative",
    width: 150,
    height: 150,
    textAlign: "center",
    mx: "auto",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
  })
);

function InnerStatusIcon() {
  const status = useAtomValue(statusAtom);
  const isProcessing = useAtomValue(isProcessingAtom);

  if (isProcessing) {
    return <ProcessingIcon />;
  }

  if (status === "connected") {
    return <ConnectedIcon />;
  }

  return <DisconnectedIcon />;
}

const DragRegion = styled(Box)(({ theme }) => ({
  position: "absolute",
  inset: 0,
}));

export default function StatusIcon() {
  return (
    <IconContainer>
      <BackgroundIcon />
      <InnerStatusIcon />
      <DragRegion data-tauri-drag-region />
    </IconContainer>
  );
}
