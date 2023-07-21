import { GppBad, VerifiedUser as VerifiedIcon } from "@mui/icons-material";
import {
  Box,
  Button,
  CircularProgress,
  Tooltip,
  styled,
  useTheme,
} from "@mui/material";
import { useAtomValue, useSetAtom } from "jotai";
import { BeatLoader } from "react-spinners";
import {
  openGatewaySwitcherAtom,
  selectedGatewayAtom,
} from "../../atoms/gateway";
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

const ConnectedIcon = () => {
  const selectedGateway = useAtomValue(selectedGatewayAtom);
  const openGatewaySwitcher = useSetAtom(openGatewaySwitcherAtom);

  return (
    <Box
      sx={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
      }}
    >
      <VerifiedIcon
        sx={{
          fontSize: 70,
          color: (theme) => theme.palette.success.main,
        }}
      />
      <Tooltip title={`Connected to ${selectedGateway?.name}`}>
        <Button
          sx={{
            position: "relative",
            zIndex: 1,
            fontSize: "0.75rem",
            fontWeight: "bold",
            display: "block",
            width: 100,
            mt: 0.2,
            padding: 0.2,
            overflow: "hidden",
            textOverflow: "ellipsis",
          }}
          size="small"
          color="success"
          onClick={openGatewaySwitcher}
        >
          {selectedGateway?.name}
        </Button>
      </Tooltip>
    </Box>
  );
};

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

export default function StatusIcon() {
  return (
    <IconContainer>
      <BackgroundIcon />
      <InnerStatusIcon />
    </IconContainer>
  );
}
